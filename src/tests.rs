use crate::{agent::Agent, config::Config, provider, store::Store, telegram, tools};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::TcpListener,
    path::Path,
};

#[test]
fn prior_memory_database_gets_status_column_without_losing_rows() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.db");
    let legacy = rusqlite::Connection::open(&path).unwrap();
    legacy.execute_batch("CREATE TABLE memories(id INTEGER PRIMARY KEY,session TEXT NOT NULL,body TEXT NOT NULL,source TEXT NOT NULL,created INTEGER NOT NULL DEFAULT(unixepoch())); INSERT INTO memories(session,body,source) VALUES('cli','keeps my notes','legacy');").unwrap();
    drop(legacy);
    let upgraded = Store::open(&path).unwrap();
    let rows = upgraded.memories("cli", "notes").unwrap();
    assert_eq!(rows[0]["memory"], "keeps my notes");
    assert_eq!(rows[0]["user_feedback"], "unrated");
    let status: String = upgraded
        .0
        .query_row("SELECT status FROM memories WHERE id=1", [], |r| r.get(0))
        .unwrap();
    assert_eq!(status, "active");
}

#[test]
fn character_profile_is_installed_by_default_and_user_edits_survive_restart() {
    let dir = tempfile::tempdir().unwrap();
    let config = config(dir.path());
    std::fs::create_dir_all(&config.state_dir).unwrap();
    let profile = config.state_dir.join("MORNYE_PROFILE.md");
    let first = Agent::new(config.clone()).unwrap();
    let initial = std::fs::read_to_string(&profile).unwrap();
    assert_eq!(initial, crate::agent::DEFAULT_CHARACTER_PROFILE);
    assert!(initial.contains("Startorch Academy"));
    drop(first);

    std::fs::write(&profile, "User-customized Mornye profile.\n").unwrap();
    let second = Agent::new(config).unwrap();
    assert_eq!(
        std::fs::read_to_string(profile).unwrap(),
        "User-customized Mornye profile.\n"
    );
    drop(second);
}

fn config(dir: &Path) -> Config {
    Config {
        state_dir: dir.join("state"),
        workspace: dir.to_owned(),
        api_key_env: "PATH".into(),
        auto_approve: false,
        ..Config::default()
    }
}

fn agent(dir: &Path, base: String) -> Agent {
    let mut config = config(dir);
    std::fs::create_dir_all(&config.state_dir).unwrap();
    config.api_base = base;
    Agent::new(config).unwrap()
}

fn mock(messages: Vec<Value>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for message in messages {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0u8; 4096];
            loop {
                let n = socket.read(&mut buffer).unwrap();
                assert!(n > 0);
                request.extend_from_slice(&buffer[..n]);
                if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..end]);
                    let len: usize = headers
                        .lines()
                        .find_map(|line| {
                            line.to_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse().unwrap())
                        })
                        .unwrap();
                    if request.len() >= end + 4 + len {
                        let sent: Value =
                            serde_json::from_slice(&request[end + 4..end + 4 + len]).unwrap();
                        assert!(sent["messages"].is_array());
                        break;
                    }
                }
            }
            let finish = if message["tool_calls"].is_array() {
                "tool_calls"
            } else {
                "stop"
            };
            let body = json!({"choices":[{"message":message,"finish_reason":finish}]}).to_string();
            write!(socket,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
        }
    });
    format!("http://{addr}")
}

fn call(name: &str, args: Value) -> Value {
    json!({"role":"assistant","content":null,"tool_calls":[{"id":"call-1","type":"function","function":{"name":name,"arguments":args.to_string()}}]})
}

#[tokio::test]
async fn default_auto_approval_runs_consecutive_shell_commands_without_prompts() {
    let dir = tempfile::tempdir().unwrap();
    let mut second = call(
        "shell",
        json!({"command":"echo second","purpose":"Read-only diagnostic"}),
    );
    second["tool_calls"][0]["id"] = json!("call-2");
    let base = mock(vec![
        call(
            "shell",
            json!({"command":"echo first","purpose":"Read-only diagnostic"}),
        ),
        second,
        json!({"role":"assistant","content":"Diagnostics complete."}),
    ]);
    let mut c = config(dir.path());
    c.auto_approve = Config::default().auto_approve;
    c.api_base = base;
    std::fs::create_dir_all(&c.state_dir).unwrap();
    let a = Agent::new(c).unwrap();
    assert_eq!(
        a.input("cli", "Check system status").await.unwrap(),
        "Diagnostics complete."
    );
    assert!(a.store.pending("cli").unwrap().is_none());
    for (call_id, output) in [("call-1", "first"), ("call-2", "second")] {
        let action = a.store.action("cli", call_id).unwrap().unwrap();
        assert_eq!(action["status"], "finished");
        let result: Value = serde_json::from_str(action["result"].as_str().unwrap()).unwrap();
        assert_eq!(result["success"], true);
        assert!(result["stdout"].as_str().unwrap().contains(output));
    }
    assert_eq!(
        a.input("cli", "/retry").await.unwrap(),
        "No unfinished turn."
    );
}

#[test]
fn existing_configs_default_to_auto_approval_and_can_opt_into_manual_review() {
    let default: Config = toml::from_str("model = 'existing-model'").unwrap();
    assert!(default.auto_approve);
    let manual: Config = toml::from_str("auto_approve = false").unwrap();
    assert!(!manual.auto_approve);
}

#[tokio::test]
async fn auto_approval_resumes_a_previously_pending_action_once_after_restart() {
    let dir = tempfile::tempdir().unwrap();
    let base = mock(vec![
        call("write_file", json!({"path":"resumed.txt","content":"done"})),
        json!({"role":"assistant","content":"Resumed."}),
    ]);
    let a = agent(dir.path(), base.clone());
    assert!(
        a.input("cli", "Write the file")
            .await
            .unwrap()
            .contains("REVIEW REQUIRED")
    );
    assert!(!dir.path().join("resumed.txt").exists());
    let mut c = a.config.clone();
    drop(a);
    c.auto_approve = true;
    let a = Agent::new(c).unwrap();
    assert_eq!(a.input("cli", "Continue").await.unwrap(), "Resumed.");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("resumed.txt")).unwrap(),
        "done"
    );
    assert!(a.store.pending("cli").unwrap().is_none());
    assert_eq!(
        a.input("cli", "/retry").await.unwrap(),
        "No unfinished turn."
    );
}

#[tokio::test]
async fn approval_is_exact_single_use_and_survives_restart() {
    let dir = tempfile::tempdir().unwrap();
    let base = mock(vec![
        call(
            "write_file",
            json!({"path":"result.txt","content":"approved"}),
        ),
        json!({"role":"assistant","content":"File written."}),
    ]);
    let a = agent(dir.path(), base.clone());
    assert!(
        a.input("cli", "Create the file")
            .await
            .unwrap()
            .contains("REVIEW REQUIRED")
    );
    assert!(!dir.path().join("result.txt").exists());
    let id = a.store.pending("cli").unwrap().unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(
        a.input("cli", "yes")
            .await
            .unwrap()
            .contains("REVIEW REQUIRED")
    );
    assert!(a.input("cli", "/approve wrong").await.is_err());
    assert!(a.input("other", &format!("/approve {id}")).await.is_err());
    drop(a);
    let a = agent(dir.path(), base);
    a.input("cli", &format!("/approve {id}")).await.unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path().join("result.txt")).unwrap(),
        "approved"
    );
    assert!(a.input("cli", &format!("/approve {id}")).await.is_err());
}

#[tokio::test]
async fn changed_file_cannot_be_deleted_with_old_approval() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("keep.txt"), "before").unwrap();
    let a = agent(
        dir.path(),
        mock(vec![
            call("delete_file", json!({"path":"keep.txt"})),
            json!({"role":"assistant","content":"Action failed."}),
        ]),
    );
    a.input("cli", "Delete keep.txt").await.unwrap();
    let id = a.store.pending("cli").unwrap().unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    std::fs::write(dir.path().join("keep.txt"), "changed").unwrap();
    a.input("cli", &format!("/approve {id}")).await.unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path().join("keep.txt")).unwrap(),
        "changed"
    );
    assert!(
        a.input("cli", "/status")
            .await
            .unwrap()
            .contains("File changed after review")
    );
}

#[tokio::test]
async fn denied_shell_never_runs() {
    let dir = tempfile::tempdir().unwrap();
    let a = agent(
        dir.path(),
        mock(vec![
            call(
                "shell",
                json!({"command":"echo bad > never.txt","purpose":"test denial"}),
            ),
            json!({"role":"assistant","content":"Declined."}),
        ]),
    );
    a.input("cli", "Run command").await.unwrap();
    let id = a.store.pending("cli").unwrap().unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    a.input("cli", &format!("/deny {id}")).await.unwrap();
    assert!(!dir.path().join("never.txt").exists());
}

#[test]
fn crash_recovery_marks_execution_uncertain() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let store = Store::open(&path).unwrap();
    store.0.execute("INSERT INTO actions(id,session,call_id,tool,args,status) VALUES('a','cli','c','shell','{}','executing')",[]).unwrap();
    drop(store);
    let store = Store::open(&path).unwrap();
    let action = store.action("cli", "c").unwrap().unwrap();
    assert_eq!(action["status"], "uncertain");
    assert!(action["result"].as_str().unwrap().contains("not replayed"));
}

#[tokio::test]
async fn memories_are_scoped_and_forget_removes_search_result() {
    let dir = tempfile::tempdir().unwrap();
    let a = agent(dir.path(), "https://unused.invalid".into());
    a.store
        .append("cli", &json!({"role":"user","content":"I like Rust"}))
        .unwrap();
    tools::execute(
        &a.store,
        &a.config,
        "cli",
        "memory_save",
        &json!({"text":"likes Rust","source":"user explicitly said"}),
    )
    .await
    .unwrap();
    tools::execute(
        &a.store,
        &a.config,
        "cli",
        "memory_save",
        &json!({"text":"likes Rust", "source":"duplicate claim"}),
    )
    .await
    .unwrap();
    let source: String = a
        .store
        .0
        .query_row("SELECT source FROM memories WHERE id=1", [], |r| r.get(0))
        .unwrap();
    let source: Value = serde_json::from_str(&source).unwrap();
    assert_eq!(source["user_message_id"], 1);
    assert_eq!(source["verified"], false);
    assert_eq!(
        a.store
            .memories("cli", "Rust")
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(
        a.store
            .memories("other", "Rust")
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(a.input("other", "/forget 1").await.is_err());
    a.input("cli", "/forget 1").await.unwrap();
    assert!(
        a.store
            .memories("cli", "Rust")
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn memory_correction_preserves_lineage_and_removes_false_fact_from_retrieval() {
    let dir = tempfile::tempdir().unwrap();
    let a = agent(dir.path(), "https://unused.invalid".into());
    a.store
        .append(
            "discord:42:10",
            &json!({"role":"user","content":"I moved to Bandung"}),
        )
        .unwrap();
    tools::execute(
        &a.store,
        &a.config,
        "discord:42:10",
        "memory_save",
        &json!({"text":"Lives in Jakarta","source":"model inferred it"}),
    )
    .await
    .unwrap();
    a.store.rate_memory("discord:42:10", 1, "wrong").unwrap();
    let new_id = a
        .store
        .correct_memory("discord:42:10", 1, "Lives in Bandung")
        .unwrap();
    assert_eq!(new_id, 2);
    assert!(
        a.store
            .memories("discord:42:10", "Jakarta")
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty()
    );
    let results = a.store.memories("discord:42:10", "Bandung").unwrap();
    assert_eq!(results[0]["memory"], "Lives in Bandung");
    assert_eq!(results[0]["user_feedback"], "unrated");
    let source: Value = serde_json::from_str(results[0]["source"].as_str().unwrap()).unwrap();
    assert_eq!(source["kind"], "explicit_user_correction");
    assert_eq!(source["corrects_memory_id"], 1);
    let lineage: (String, String) = a
        .store
        .0
        .query_row(
            "SELECT event,old_text FROM memory_history WHERE memory_id=1 AND event='corrected'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(lineage, ("corrected".into(), "Lives in Jakarta".into()));
    assert!(
        a.store
            .correct_memory("other", 1, "Lives in Paris")
            .is_err()
    );
}

#[tokio::test]
async fn learned_skills_remain_inactive_until_human_promotes() {
    let dir = tempfile::tempdir().unwrap();
    let a = agent(dir.path(), "https://unused.invalid".into());
    tools::execute(
        &a.store,
        &a.config,
        "cli",
        "skill_propose",
        &json!({"text":"Run tests before deploy","evidence":"action 12 exit code 0"}),
    )
    .await
    .unwrap();
    let evidence: String = a
        .store
        .0
        .query_row("SELECT evidence FROM skills WHERE id=1", [], |r| r.get(0))
        .unwrap();
    let evidence: Value = serde_json::from_str(&evidence).unwrap();
    assert_eq!(evidence["model_claim"], "action 12 exit code 0");
    assert_eq!(evidence["runtime_action_records"], json!([]));
    assert_eq!(evidence["evaluation"], "not_evaluated");
    assert!(
        tools::execute(&a.store, &a.config, "cli", "skill_search", &json!({}))
            .await
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty()
    );
    a.input("cli", "/promote 1").await.unwrap();
    assert_eq!(
        tools::execute(&a.store, &a.config, "cli", "skill_search", &json!({}))
            .await
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn telegram_rejects_other_users_groups_and_bots() {
    let mut update = json!({"message":{"from":{"id":42,"is_bot":false},"chat":{"id":42,"type":"private"},"text":"/approve x"}});
    assert!(telegram::authorized(&update, 42));
    assert!(!telegram::authorized(&update, 43));
    update["message"]["chat"]["type"] = json!("group");
    assert!(!telegram::authorized(&update, 42));
    update["message"]["chat"]["type"] = json!("private");
    update["message"]["from"]["is_bot"] = json!(true);
    assert!(!telegram::authorized(&update, 42));
}

#[test]
fn model_cannot_forge_policy_or_approval_fields() {
    let dir = tempfile::tempdir().unwrap();
    let c = config(dir.path());
    assert!(tools::needs_approval("shell"));
    assert!(tools::needs_approval("unknown"));
    assert!(
        tools::prepare(
            &c,
            "shell",
            &json!({"command":"echo hi","purpose":"test","approved":true})
        )
        .is_err()
    );
    assert!(tools::prepare(&c, "approve", &json!({"id":"x"})).is_err());
    assert!(provider::validate(&json!({"role":"user","content":"/approve x"})).is_err());
    assert!(provider::validate(&json!({"role":"assistant","tool_calls":[{"id":"x","type":"function","function":{"name":"shell","arguments":"{}"}},{"id":"x","type":"function","function":{"name":"shell","arguments":"{}"}}]})).is_err());
}

#[tokio::test]
async fn output_is_bounded_and_failed_commands_are_not_success() {
    let bytes = vec![b'x'; tools::OUTPUT_LIMIT * 3];
    let (output, truncated) = tools::drain(bytes.as_slice()).await.unwrap();
    assert_eq!(output.len(), tools::OUTPUT_LIMIT);
    assert!(truncated);
    let dir = tempfile::tempdir().unwrap();
    let a = agent(dir.path(), "https://unused.invalid".into());
    let prepared = tools::prepare(
        &a.config,
        "shell",
        &json!({"command":"exit 7","purpose":"test failure recording"}),
    )
    .unwrap();
    let result = tools::execute(&a.store, &a.config, "cli", "shell", &prepared)
        .await
        .unwrap();
    assert_eq!(result["exit_code"], 7);
    assert_eq!(result["success"], false);
}

#[test]
fn context_trimming_preserves_complete_tool_groups() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("test.db")).unwrap();
    for i in 0..10 {
        store
            .append(
                "cli",
                &json!({"role":"user","content":format!("question {i}")}),
            )
            .unwrap();
        store
            .append("cli", &call("list_files", json!({"path":"."})))
            .unwrap();
        store
            .append(
                "cli",
                &json!({"role":"tool","tool_call_id":"call-1","content":"[]"}),
            )
            .unwrap();
        store
            .append("cli", &json!({"role":"assistant","content":"done"}))
            .unwrap();
    }
    let history = store.history("cli", 700).unwrap();
    assert_eq!(history[0]["role"], "user");
    assert!(crate::agent::unresolved(&history).is_empty());
    assert!(history.len() < 40);
}

#[test]
fn syntax_graph_updates_changed_files_and_flags_deleted_sources() {
    let dir = tempfile::tempdir().unwrap();
    let a = agent(dir.path(), "https://unused.invalid".into());
    let source = dir.path().join("sample.rs");
    std::fs::write(&source, "fn caller() { callee(); } fn callee() {}\n").unwrap();
    std::fs::write(
        dir.path().join("sample.ts"),
        "export function hello() { greet(); }\n",
    )
    .unwrap();
    let first = crate::graph::index(&a.store, &a.config, dir.path()).unwrap();
    assert_eq!(first["parsed"], 2);
    let edges = crate::graph::search(&a.store, "callee").unwrap();
    assert!(
        edges["edges"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["source"] == "caller" && e["relation"] == "calls" && e["stale"] == false)
    );
    assert!(
        !crate::graph::search(&a.store, "greet").unwrap()["edges"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        crate::graph::index(&a.store, &a.config, dir.path()).unwrap()["unchanged"],
        2
    );
    std::fs::write(&source, "fn replacement() {}\n").unwrap();
    assert!(
        crate::graph::search(&a.store, "callee").unwrap()["edges"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["stale"] == true)
    );
    crate::graph::index(&a.store, &a.config, dir.path()).unwrap();
    assert!(
        crate::graph::search(&a.store, "callee").unwrap()["edges"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    std::fs::remove_file(&source).unwrap();
    assert_eq!(
        crate::graph::search(&a.store, "replacement").unwrap()["edges"][0]["stale"],
        true
    );
}

#[tokio::test]
async fn expired_approval_does_not_execute() {
    let dir = tempfile::tempdir().unwrap();
    let a = agent(
        dir.path(),
        mock(vec![call(
            "write_file",
            json!({"path":"expired.txt","content":"no"}),
        )]),
    );
    a.input("cli", "Create a file").await.unwrap();
    let id = a.store.pending("cli").unwrap().unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    a.store
        .0
        .execute(
            "UPDATE actions SET created=unixepoch()-86401 WHERE id=?1",
            [&id],
        )
        .unwrap();
    assert!(a.input("cli", &format!("/approve {id}")).await.is_err());
    assert!(!dir.path().join("expired.txt").exists());
}

#[tokio::test]
async fn command_timeout_is_recorded_as_failure() {
    let dir = tempfile::tempdir().unwrap();
    let a = agent(dir.path(), "https://unused.invalid".into());
    #[cfg(windows)]
    let command = "Start-Sleep -Seconds 5";
    #[cfg(not(windows))]
    let command = "exec sleep 5";
    let result = tools::execute(
        &a.store,
        &a.config,
        "cli",
        "shell",
        &json!({"command":command,"cwd":dir.path(),"timeout_seconds":1}),
    )
    .await
    .unwrap();
    assert_eq!(result["timed_out"], true);
    assert_eq!(result["success"], false);
}
