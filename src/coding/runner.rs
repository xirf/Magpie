use super::{local_log, store::Job, Service};
use serde_json::{json, Value};
use std::io::Write;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::watch;
use tokio::time::Instant;

const MAX_EVENT: usize = 256 * 1024;

fn process(command: &mut Command) {
    command.kill_on_drop(true).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(unix)] {
        use std::os::unix::process::CommandExt;
        command.as_std_mut().process_group(0);
    }
}

async fn terminate(child: &mut Child) {
    #[cfg(unix)]
    if let Some(id) = child.id() {
        // The child is its own process group, including shell descendants.
        let _ = Command::new("kill").args(["-KILL","--",&format!("-{id}")])
            .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().await;
    }
    let _ = child.kill().await;
    let _ = child.wait().await;
}

async fn cancelled(receiver: &mut watch::Receiver<bool>) {
    loop {
        if *receiver.borrow() { return; }
        if receiver.changed().await.is_err() { return; }
    }
}

async fn plain(mut command: Command, receiver: &mut watch::Receiver<bool>, deadline: Instant) -> Result<String,String> {
    if *receiver.borrow() { return Err("Task cancelled".into()); }
    process(&mut command);
    let mut child = command.spawn().map_err(|_| "Cannot start task command")?;
    let mut output = child.stdout.take().ok_or("Missing command output")?;
    let future = async {
        let mut bytes = Vec::new();
        let mut chunk = [0u8;4096];
        loop {
            let n = output.read(&mut chunk).await.map_err(|_| "Cannot read task command output")?;
            if n == 0 { break; }
            let available = 65536usize.saturating_sub(bytes.len());
            bytes.extend_from_slice(&chunk[..n.min(available)]);
        }
        let status = child.wait().await.map_err(|_| "Cannot wait for task command")?;
        if !status.success() { return Err("Task command failed (see configured checks or repository)".into()); }
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    };
    let result = tokio::select! {
        result = future => result,
        _ = cancelled(receiver) => Err("Task cancelled".into()),
        _ = tokio::time::sleep_until(deadline) => Err("Task deadline exceeded".into()),
    };
    if result.is_err() { terminate(&mut child).await; }
    result
}

/// Reject oversized JSON frames before allocating unbounded output.
async fn frame<R: tokio::io::AsyncBufRead + Unpin>(reader: &mut R) -> Result<Option<Value>,String> {
    let mut bytes = Vec::new();
    loop {
        let chunk = reader.fill_buf().await.map_err(|_| "Cannot read Zot events")?;
        if chunk.is_empty() {
            if bytes.is_empty() { return Ok(None); }
            return Err("Zot ended with an incomplete JSON event".into());
        }
        let end = chunk.iter().position(|b| *b == b'\n').map(|n|n+1);
        let n = end.unwrap_or(chunk.len());
        if bytes.len()+n > MAX_EVENT { return Err("Zot event exceeds output limit".into()); }
        bytes.extend_from_slice(&chunk[..n]);
        reader.consume(n);
        if end.is_some() {
            if bytes.iter().all(|b|b.is_ascii_whitespace()) { bytes.clear(); continue; }
            return serde_json::from_slice(&bytes).map(Some).map_err(|_| "Invalid Zot JSON event".into());
        }
    }
}

#[derive(Default)]
struct Tokens { input: u64, output: u64 }

impl Tokens {
    fn update(&mut self, event: &Value) -> u64 {
        if let Some(cumulative) = event.get("cumulative").filter(|v|v.is_object()) {
            self.input = cumulative["input"].as_u64().unwrap_or(self.input);
            self.output = cumulative["output"].as_u64().unwrap_or(self.output);
        } else {
            let input = event["input"].as_u64().unwrap_or(0);
            let output = event["output"].as_u64().unwrap_or(0);
            if event["cumulative"].as_bool().unwrap_or(false) {
                self.input = input;
                self.output = output;
            } else {
                self.input = self.input.saturating_add(input);
                self.output = self.output.saturating_add(output);
            }
        }
        self.input.saturating_add(self.output)
    }
}

pub(super) async fn run(service: &Service, job: &Job, mut receiver: watch::Receiver<bool>) -> Result<bool,String> {
    let config = &service.config;
    let project = config.projects.get(&job.project).ok_or("Project is no longer configured")?;
    let deadline = Instant::now()+Duration::from_secs(config.timeout_seconds);
    let worktree = service.worktree(job.id);
    let mut log = local_log(&config.state_dir.join("logs").join(format!("{}.jsonl",job.id)))?;

    let mut git = Command::new("git");
    git.arg("-C").arg(&project.path).args(["rev-parse","--show-toplevel"]);
    let root = plain(git,&mut receiver,deadline).await?;
    let root = std::fs::canonicalize(root.trim()).map_err(|_| "Cannot identify project Git root")?;
    if root != project.path { return Err("Configured project must be the Git repository root".into()); }
    let mut git = Command::new("git");
    git.arg("-C").arg(&project.path).args(["worktree","add","-b"])
        .arg(format!("magpie/code-{}",job.id)).arg(&worktree).arg("HEAD");
    plain(git,&mut receiver,deadline).await?;

    let api_key = std::env::var(&config.api_key_env).map_err(|_| "DeepSeek API key is unavailable")?;
    if api_key.is_empty() { return Err("DeepSeek API key is unavailable".into()); }
    let mut command = Command::new(&config.zot_binary);
    command.args(["rpc","--provider","deepseek","--model"]).arg(&config.model)
        .arg("--base-url").arg(&config.base_url)
        .arg("--cwd").arg(&worktree)
        .arg("--max-steps").arg(config.max_steps.to_string())
        .env("DEEPSEEK_API_KEY",api_key)
        .env("ZOT_HOME",config.state_dir.join("zot").join(job.id.to_string()))
        .env("TMPDIR",config.state_dir.join("tmp"))
        .current_dir(&worktree);
    process(&mut command);
    command.stdin(Stdio::piped());
    let mut child = command.spawn().map_err(|_| "Cannot start Zot; check coding.zot_binary")?;
    let mut input = child.stdin.take().ok_or("Missing Zot input")?;
    let output = child.stdout.take().ok_or("Missing Zot output")?;
    let mut reader = BufReader::new(output);
    let prompt = json!({"id":"task","type":"prompt","message": format!(
        "Work only in this task's Git worktree. Do not push, merge, deploy, or modify external files. \
        Implement this task and explain the changes and verification:\n{}",job.prompt)}).to_string()+"\n";
    let future = async {
        input.write_all(prompt.as_bytes()).await.map_err(|_| "Cannot send Zot prompt")?;
        input.flush().await.map_err(|_| "Cannot flush Zot prompt")?;
        let mut tokens = Tokens::default();
        let mut saw_answer = false;
        let mut count = 0u32;
        loop {
            let event = frame(&mut reader).await?.ok_or("Zot exited without a completion event")?;
            count += 1;
            if count > 10_000 { return Err("Task event limit exceeded".into()); }
            let kind = event["type"].as_str().unwrap_or("unknown");
            // Store metadata only: never copy model text, credentials, or shell output to Discord/logs.
            if matches!(kind,"turn_start"|"tool_call"|"tool_result"|"usage"|"turn_end"|"done") {
                let entry = json!({"type":kind,"step":event.get("step"),
                    "tool":event.get("name").and_then(|v|v.as_str()).map(|s|s.chars().take(64).collect::<String>()),
                    "input":event.get("input"),"output":event.get("output"),"stop":event.get("stop")});
                writeln!(log,"{entry}").map_err(|_| "Cannot write task log")?;
            }
            match kind {
                "response" if event["success"] == false => return Err("Zot rejected the task".into()),
                "error" => return Err("Zot reported a provider or agent error".into()),
                "turn_end" if matches!(event["stop"].as_str(),Some("error"|"aborted"|"length")) => {
                    return Err("Zot task ended before successful completion".into());
                }
                "usage" if tokens.update(&event) >= config.max_tokens => return Err("Task token limit reached".into()),
                "assistant_message" => saw_answer = true,
                "done" => {
                    if !saw_answer { return Err("Zot finished without an assistant result".into()); }
                    break;
                }
                _ => {}
            }
        }
        drop(input);
        let status = tokio::time::timeout(Duration::from_secs(5),child.wait()).await
            .map_err(|_| "Zot did not shut down after completion")?
            .map_err(|_| "Cannot wait for Zot")?;
        if !status.success() { return Err("Zot exited unsuccessfully".into()); }
        Ok(())
    };
    let result = tokio::select! {
        result = future => result,
        _ = cancelled(&mut receiver) => Err("Task cancelled".into()),
        _ = tokio::time::sleep_until(deadline) => Err("Task deadline exceeded".into()),
    };
    if result.is_err() { terminate(&mut child).await; }
    result?;
    if let Some(check) = project.check.as_ref().filter(|s|!s.trim().is_empty()) {
        if !service.store.finish(job.id,"verifying","Running configured checks").map_err(|_| "Cannot update task state")? {
            return Err("Task cancelled".into());
        }
        let mut command = Command::new("sh");
        command.args(["-c",check]).current_dir(&worktree)
            .env("TMPDIR",config.state_dir.join("tmp"));
        plain(command,&mut receiver,deadline).await?;
        writeln!(log,"{}",json!({"type":"verification","passed":true})).map_err(|_| "Cannot write verification result")?;
        Ok(true)
    } else { Ok(false) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn rejects_oversized_and_incomplete_events() {
        let bytes = vec![b'x';MAX_EVENT+1];
        assert!(frame(&mut BufReader::new(&bytes[..])).await.is_err());
        assert!(frame(&mut BufReader::new(&b"{\"type\":\"done\"}"[..])).await.is_err());
        assert_eq!(frame(&mut BufReader::new(&b"{\"type\":\"done\"}\n"[..])).await.unwrap().unwrap()["type"],"done");
    }
    #[test]
    fn accounts_for_incremental_and_cumulative_tokens() {
        let mut t = Tokens::default();
        assert_eq!(t.update(&json!({"input":100,"output":10})),110);
        assert_eq!(t.update(&json!({"input":200,"output":20})),330);
        assert_eq!(t.update(&json!({"cumulative":{"input":300,"output":30}})),330);
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn fake_harness_runs_in_worktree_and_checks_real_result() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("magpie-worker-test-{}-{}",std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let repo = root.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        for args in [vec!["init"],vec!["-c","user.name=Test","-c","user.email=test@example.invalid",
            "commit","--allow-empty","-m","fixture"]] {
            assert!(std::process::Command::new("git").current_dir(&repo).args(args)
                .stdout(Stdio::null()).stderr(Stdio::null()).status().unwrap().success());
        }
        let harness = root.join("fake-zot");
        std::fs::write(&harness,concat!("#!/bin/sh\n",
            "read -r prompt\n",
            "printf 'implemented' > result.txt\n",
            "printf '%s\\n' '{\"type\":\"response\",\"id\":\"task\",\"success\":true}' ",
            "'{\"type\":\"assistant_message\",\"content\":[]}' '{\"type\":\"done\"}'\n")).unwrap();
        std::fs::set_permissions(&harness,std::fs::Permissions::from_mode(0o700)).unwrap();
        std::env::set_var("MAGPIE_CODING_TEST_API_KEY","fixture-not-a-real-key");
        let mut config = super::super::CodingSettings {
            enabled:true, zot_binary:harness, state_dir:root.join("state"),
            api_key_env:"MAGPIE_CODING_TEST_API_KEY".into(), allowed_channels:vec!["123".into()],
            ..Default::default()
        };
        config.projects.insert("fixture".into(),super::super::Project {
            path:repo,check:Some("test \"$(cat result.txt)\" = implemented".into())
        });
        let service = Service::new(config).unwrap();
        let id = service.store.enqueue("fixture","implement fixture","123","456").unwrap();
        let job = service.store.claim().unwrap().unwrap();
        let (_sender,receiver) = watch::channel(false);
        assert!(run(&service,&job,receiver).await.unwrap());
        assert_eq!(std::fs::read_to_string(service.worktree(id).join("result.txt")).unwrap(),"implemented");
        assert_eq!(service.store.get(id).unwrap().unwrap().status,"verifying");
        // Fail the configured verification even if the model claims completion.
        let mut failure_config = service.config.clone();
        failure_config.projects.get_mut("fixture").unwrap().check = Some("exit 1".into());
        let failure_service = Service::new(failure_config).unwrap();
        let id = failure_service.store.enqueue("fixture","second fixture","123","456").unwrap();
        let job = failure_service.store.claim().unwrap().unwrap();
        let (_sender,receiver) = watch::channel(false);
        assert!(run(&failure_service,&job,receiver).await.is_err());
        assert!(failure_service.worktree(id).join("result.txt").exists());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn cancellation_terminates_running_shell_group() {
        let (sender,mut receiver) = watch::channel(false);
        let mut command = Command::new("sh");
        command.args(["-c","sleep 60 & wait"]);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let _ = sender.send(true);
        });
        let start = Instant::now();
        assert!(plain(command,&mut receiver,Instant::now()+Duration::from_secs(5)).await.is_err());
        assert!(start.elapsed() < Duration::from_secs(3));
    }
}
