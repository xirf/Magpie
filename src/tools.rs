use crate::{config::Config, store::Store};
use anyhow::{Context, Result, bail, ensure};
use rusqlite::params;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::io::{AsyncRead, AsyncReadExt};

const MAX_FILE: u64 = 128 * 1024;
pub const OUTPUT_LIMIT: usize = 16 * 1024;

pub fn schemas() -> Value {
    let specs = [
        (
            "code_index",
            "Build or refresh a bounded Rust/TypeScript syntax graph for a file or directory. Only Mornye's index is written; source files are unchanged.",
            json!({"path":{"type":"string"}}),
            vec!["path"],
        ),
        (
            "code_search",
            "Find definitions, calls, and imports in the syntax graph. Check stale and parse_errors fields before relying on edges.",
            json!({"query":{"type":"string"}}),
            vec!["query"],
        ),
        (
            "read_file",
            "Read a UTF-8 file on the host. Output is capped.",
            json!({"path":{"type":"string"}}),
            vec!["path"],
        ),
        (
            "list_files",
            "List a directory, up to 200 entries.",
            json!({"path":{"type":"string"}}),
            vec!["path"],
        ),
        (
            "write_file",
            "Create or replace a UTF-8 file. Executes automatically unless runtime approval mode is manual.",
            json!({"path":{"type":"string"},"content":{"type":"string"}}),
            vec!["path", "content"],
        ),
        (
            "delete_file",
            "Delete one file. Directories are rejected. Executes automatically unless runtime approval mode is manual.",
            json!({"path":{"type":"string"}}),
            vec!["path"],
        ),
        (
            "shell",
            "Execute an exact host shell command for tests, SSH, deployment, or other operations. Executes automatically unless runtime approval mode is manual. Return exit status and bounded stdout/stderr. Do not detach processes or embed secrets.",
            json!({"command":{"type":"string"},"purpose":{"type":"string"}}),
            vec!["command", "purpose"],
        ),
        (
            "memory_search",
            "Search personal memories belonging to this conversation owner.",
            json!({"query":{"type":"string"}}),
            vec!["query"],
        ),
        (
            "memory_save",
            "Save one useful fact or explicit preference, including its source. Never store credentials or treat inferred facts as user instructions.",
            json!({"text":{"type":"string"},"source":{"type":"string"}}),
            vec!["text", "source"],
        ),
        (
            "skill_propose",
            "Save a CANDIDATE reusable procedure with evidence. It is inactive until the human promotes it. Describe verification and rollback; do not claim unperformed tests.",
            json!({"text":{"type":"string"},"evidence":{"type":"string"}}),
            vec!["text", "evidence"],
        ),
        (
            "skill_search",
            "Read approved skills for this owner. Skills do not authorize tools.",
            json!({}),
            vec![],
        ),
        (
            "schedule",
            "Schedule a one-time future task. delay_seconds is elapsed seconds. The task runs only while the service is running. Runtime approval mode applies to scheduling and future actions.",
            json!({"prompt":{"type":"string"},"delay_seconds":{"type":"integer","minimum":60,"maximum":31536000}}),
            vec!["prompt", "delay_seconds"],
        ),
    ];
    Value::Array(specs.into_iter().map(|(name, description, properties, required)| json!({"type":"function","function":{"name":name,"description":description,"parameters":{"type":"object","properties":properties,"required":required,"additionalProperties":false}}})).collect())
}

pub fn needs_approval(name: &str) -> bool {
    // Unknown tools fail closed, never become an implicitly trusted shell path.
    !matches!(
        name,
        "read_file"
            | "list_files"
            | "memory_search"
            | "memory_save"
            | "skill_propose"
            | "skill_search"
            | "code_index"
            | "code_search"
    )
}

pub fn text<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args[key]
        .as_str()
        .with_context(|| format!("Missing string argument: {key}"))
}

fn path(config: &Config, value: &str) -> Result<PathBuf> {
    let input = Path::new(value);
    let resolved = if input.is_absolute() {
        input.to_owned()
    } else {
        config.workspace.join(input)
    };
    // Resolve aliases before showing the user what will change; no workspace sandbox.
    if resolved.exists() {
        return Ok(std::fs::canonicalize(resolved)?);
    }
    let parent = std::fs::canonicalize(resolved.parent().context("Missing parent")?)?;
    Ok(parent.join(resolved.file_name().context("Missing filename")?))
}

fn fingerprint(path: &Path) -> Result<String> {
    if !path.exists() {
        return Ok("absent".into());
    }
    ensure!(path.is_file(), "Expected a regular file");
    ensure!(
        std::fs::metadata(path)?.len() <= MAX_FILE,
        "File exceeds 128 KiB limit; use a shell command for larger files"
    );
    Ok(format!("{:x}", Sha256::digest(std::fs::read(path)?)))
}

pub fn prepare(config: &Config, name: &str, args: &Value) -> Result<Value> {
    let schema = schemas();
    let spec = schema
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["function"]["name"] == name)
        .context("Unknown tool")?;
    let obj = args
        .as_object()
        .context("Tool arguments must be an object")?;
    let fields = &spec["function"]["parameters"]["properties"];
    for key in obj.keys() {
        ensure!(fields.get(key).is_some(), "Unknown argument {key}");
    }
    for key in spec["function"]["parameters"]["required"]
        .as_array()
        .unwrap()
    {
        let key = key.as_str().unwrap();
        if key == "delay_seconds" {
            ensure!(
                args[key].as_u64().is_some(),
                "delay_seconds must be an integer"
            );
        } else {
            text(args, key)?;
        }
    }
    ensure!(
        args.to_string().len() <= MAX_FILE as usize,
        "Tool arguments too large"
    );
    let mut args = args.clone();
    if matches!(
        name,
        "read_file" | "list_files" | "write_file" | "delete_file" | "code_index"
    ) {
        let target = path(config, text(&args, "path")?)?;
        args["path"] = json!(target);
        if matches!(name, "write_file" | "delete_file") {
            // Mornye's own state and policy cannot be changed through generic file tools.
            ensure!(
                !target.starts_with(&config.state_dir),
                "Use user commands to manage Mornye state"
            );
            args["expected_hash"] = json!(fingerprint(&target)?);
        }
    }
    if name == "shell" {
        ensure!(
            text(&args, "command")?.len() <= 8192,
            "Command exceeds 8 KiB"
        );
        args["cwd"] = json!(config.workspace);
        args["timeout_seconds"] = json!(config.command_timeout_secs);
    }
    if matches!(name, "memory_save" | "skill_propose") {
        ensure!(
            text(&args, "text")?.len() <= 4096,
            "Memory/skill exceeds 4 KiB"
        );
    }
    if name == "schedule" {
        let delay = args["delay_seconds"].as_u64().context("Invalid delay")?;
        ensure!(
            (60..=31_536_000).contains(&delay),
            "Delay must be between 60 seconds and one year"
        );
    }
    Ok(args)
}

pub async fn execute(
    store: &Store,
    config: &Config,
    session: &str,
    name: &str,
    args: &Value,
) -> Result<Value> {
    match name {
        "code_index" => crate::graph::index(store, config, Path::new(text(args, "path")?)),
        "code_search" => crate::graph::search(store, text(args, "query")?),
        "read_file" => {
            let file = std::fs::File::open(text(args, "path")?)?;
            ensure!(file.metadata()?.is_file(), "Only regular files can be read");
            let mut bytes = Vec::new();
            std::io::Read::take(file, MAX_FILE + 1).read_to_end(&mut bytes)?;
            ensure!(bytes.len() <= MAX_FILE as usize, "File exceeds 128 KiB");
            Ok(json!({"content":String::from_utf8(bytes)?}))
        }
        "list_files" => {
            let mut entries = Vec::new();
            for entry in std::fs::read_dir(text(args, "path")?)?.take(200) {
                let entry = entry?;
                entries.push(json!({"name":entry.file_name().to_string_lossy(),"directory":entry.file_type()?.is_dir()}));
            }
            Ok(json!({"entries":entries,"limit":200}))
        }
        "write_file" | "delete_file" => {
            let target = Path::new(text(args, "path")?);
            ensure!(
                fingerprint(target)? == text(args, "expected_hash")?,
                "File changed after review; request a new approval"
            );
            if name == "write_file" {
                std::fs::write(target, text(args, "content")?)?;
            } else {
                std::fs::remove_file(target)?;
            }
            Ok(json!({"status":"completed","path":target}))
        }
        "shell" => {
            run_shell(
                text(args, "command")?,
                Path::new(text(args, "cwd")?),
                args["timeout_seconds"]
                    .as_u64()
                    .context("Missing timeout")?,
                config,
            )
            .await
        }
        "memory_search" => store.memories(session, text(args, "query")?),
        "memory_save" => {
            let body = text(args, "text")?.trim();
            ensure!(!body.is_empty(), "Memory must not be empty");
            let message_id: Option<i64> = store.0.query_row("SELECT max(id) FROM messages WHERE session=?1 AND json_extract(body,'$.role')='user'", [session], |r| r.get(0))?;
            let source = json!({"model_source_claim":text(args,"source")?,"user_message_id":message_id,"verified":false}).to_string();
            store.0.execute(
                "INSERT INTO memories(session,body,source) SELECT ?1,?2,?3 WHERE NOT EXISTS(SELECT 1 FROM memories WHERE session=?1 AND body=?2)",
                params![session, body, source],
            )?;
            let id: i64 = store.0.query_row(
                "SELECT id FROM memories WHERE session=?1 AND body=?2 ORDER BY id LIMIT 1",
                params![session, body],
                |r| r.get(0),
            )?;
            Ok(json!({"memory_id":id,"verified":false}))
        }
        "skill_propose" => {
            let mut query = store.0.prepare("SELECT id,tool,status,result FROM actions WHERE session=?1 AND status IN ('finished','uncertain') ORDER BY rowid DESC LIMIT 8")?;
            let actual = query.query_map([session], |r| Ok(json!({"action_id":r.get::<_,String>(0)?,"tool":r.get::<_,String>(1)?,"status":r.get::<_,String>(2)?,"result":r.get::<_,Option<String>>(3)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;
            let evidence = json!({"model_claim":text(args,"evidence")?,"runtime_action_records":actual,"evaluation":"not_evaluated","note":"Recent action records are provenance, not proof that this procedure succeeds."}).to_string();
            store.0.execute(
                "INSERT INTO skills(session,body,evidence) VALUES(?1,?2,?3)",
                params![session, text(args, "text")?, evidence],
            )?;
            Ok(
                json!({"candidate_id":store.0.last_insert_rowid(),"status":"candidate","instruction":"Human must inspect /skills and /promote ID; no automatic activation."}),
            )
        }
        "skill_search" => {
            let mut stmt = store.0.prepare("SELECT id,body,evidence FROM skills WHERE session=?1 AND status='active' ORDER BY id DESC LIMIT 8")?;
            let rows = stmt.query_map([session],|r| Ok(json!({"id":r.get::<_,i64>(0)?,"text":r.get::<_,String>(1)?,"evidence":r.get::<_,String>(2)?})))?;
            Ok(json!(rows.collect::<rusqlite::Result<Vec<_>>>()?))
        }
        "schedule" => {
            store.0.execute(
                "INSERT INTO schedules(session,prompt,due) VALUES(?1,?2,unixepoch()+?3)",
                params![
                    session,
                    text(args, "prompt")?,
                    args["delay_seconds"].as_i64().context("Invalid delay")?
                ],
            )?;
            Ok(
                json!({"schedule_id":store.0.last_insert_rowid(),"status":"queued","runner":"telegram service"}),
            )
        }
        _ => bail!("Unknown tool"),
    }
}

pub async fn drain<R: AsyncRead + Unpin>(mut reader: R) -> std::io::Result<(String, bool)> {
    let mut kept = Vec::new();
    let mut truncated = false;
    let mut buffer = [0; 4096];
    loop {
        let n = reader.read(&mut buffer).await?;
        if n == 0 {
            break;
        }
        let remaining = OUTPUT_LIMIT.saturating_sub(kept.len());
        kept.extend_from_slice(&buffer[..n.min(remaining)]);
        truncated |= n > remaining;
    }
    Ok((String::from_utf8_lossy(&kept).into_owned(), truncated))
}

async fn run_shell(command: &str, cwd: &Path, timeout: u64, config: &Config) -> Result<Value> {
    #[cfg(windows)]
    let mut cmd = {
        let mut c = tokio::process::Command::new("powershell.exe");
        c.args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            command,
        ]);
        c
    };
    #[cfg(not(windows))]
    let mut cmd = {
        let mut c = tokio::process::Command::new("/bin/sh");
        c.args(["-c", command]);
        c
    };
    // Direct host execution, deliberately no sandbox. Avoid leaking service credentials.
    cmd.current_dir(cwd)
        .env_remove(&config.api_key_env)
        .env_remove("TELEGRAM_BOT_TOKEN")
        .env_remove("DISCORD_BOT_TOKEN")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = cmd.spawn()?;
    let stdout = child.stdout.take().context("Missing stdout")?;
    let stderr = child.stderr.take().context("Missing stderr")?;
    let out = tokio::spawn(drain(stdout));
    let err = tokio::spawn(drain(stderr));
    let status = match tokio::time::timeout(Duration::from_secs(timeout), child.wait()).await {
        Ok(result) => Some(result?),
        Err(_) => {
            child.kill().await.ok();
            child.wait().await.ok();
            None
        }
    };
    // A detached descendant can retain pipe handles. Never wait indefinitely for it.
    let mut out = out;
    let mut err = err;
    let output = tokio::time::timeout(Duration::from_secs(2), async {
        Ok::<_, anyhow::Error>(((&mut out).await??, (&mut err).await??))
    })
    .await;
    let ((stdout, out_cut), (stderr, err_cut)) = match output {
        Ok(v) => v?,
        Err(_) => {
            out.abort();
            err.abort();
            (
                ("Output unavailable: descendant retained pipe".into(), true),
                (String::new(), true),
            )
        }
    };
    Ok(
        json!({"exit_code":status.and_then(|s|s.code()),"timed_out":status.is_none(),"stdout":stdout,"stderr":stderr,
        "truncated":out_cut || err_cut,"success":status.is_some_and(|s|s.success()),
        "note": if status.is_none() { "Shell stopped; child processes may still exist. Inspect before retrying." } else { "Exit status is command evidence, not proof of deployment health." }}),
    )
}

use std::io::Read;
