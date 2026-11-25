//! The Claude account client against `zephium-fake-claude`, a stand-in CLI
//! whose behavior the system prompt picks (`FAKE_MODE=...`).
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::json;
use zephium_app::claude_account::{argv, ClaudeAccountClient, ClaudeCli};
use zephium_core::work::model::*;

fn cli() -> ClaudeCli {
    ClaudeCli {
        program: PathBuf::from(env!("CARGO_BIN_EXE_zephium-fake-claude")),
        version: (2, 1, 331),
    }
}

fn request(mode: &str) -> WorkModelRequest {
    WorkModelRequest {
        image_fence: None,
        model: WorkModelRef {
            provider: WorkModelProvider::ClaudeAccount,
            wire: WorkModelWire::AnthropicMessages,
            model: "sonnet".into(),
        },
        system: vec![WorkModelSystemBlock {
            text: format!("FAKE_MODE={mode}"),
            cache: true,
        }],
        tools: vec![WorkModelTool {
            name: "open_tab".into(),
            description: "Opens a tab.".into(),
            schema: json!({"type": "object"}),
            strict: false,
        }],
        messages: vec![WorkModelMessage::User(vec![WorkModelPart::Text(
            "Open example.com".into(),
        )])],
        max_output_tokens: 100,
        reasoning: None,
        native_search: false,
        parallel_tools: true,
        tool_choice: WorkModelToolChoice::Auto,
        volatile_tail: 0,
        cache_key: None,
    }
}

async fn call(
    client: &ClaudeAccountClient,
    request: WorkModelRequest,
) -> (Result<WorkModelOutcome, WorkModelError>, Vec<String>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    let events = move |event: WorkModelEvent| {
        sink.lock().unwrap().push(match event {
            WorkModelEvent::Text(text) => format!("text:{text}"),
            WorkModelEvent::ToolCall(call) => format!("tool:{}", call.name),
            WorkModelEvent::Search { .. } => "search".into(),
        });
    };
    let result = client.call(request, &events).await;
    let seen = seen.lock().unwrap().clone();
    (result, seen)
}

#[test]
fn argv_is_exact_and_tools_off() {
    let args = argv("sonnet", std::path::Path::new("prompt.txt"));
    let joined = args.join(" ");
    assert_eq!(args[0], "-p");
    for flag in [
        "--input-format stream-json",
        "--output-format stream-json",
        "--include-partial-messages",
        "--strict-mcp-config",
        "--disable-slash-commands",
        "--no-session-persistence",
        "--max-turns 3",
        "--model sonnet",
        "--system-prompt-file prompt.txt",
    ] {
        assert!(joined.contains(flag), "{flag}");
    }
    let at = |flag: &str| args.iter().position(|arg| arg == flag).unwrap();
    assert_eq!(args[at("--tools") + 1], "");
    assert_eq!(args[at("--setting-sources") + 1], "");
    assert!(args[at("--json-schema") + 1].contains("tool_calls"));
    assert!(!args
        .iter()
        .any(|arg| arg == "--bare" || arg.contains("Bash")));
}

#[tokio::test]
async fn text_mode_returns_end_turn() {
    let (result, seen) = call(&ClaudeAccountClient::new(cli()), request("text")).await;
    let outcome = result.unwrap();
    assert_eq!(outcome.stop, WorkModelStop::EndTurn);
    assert_eq!(seen, ["text:fake reply"]);
    assert_eq!(outcome.usage.output_tokens, 5);
    assert_eq!(outcome.usage.cost_micros, None);
}

#[tokio::test]
async fn tool_mode_emits_tool_call_event() {
    let (result, seen) = call(&ClaudeAccountClient::new(cli()), request("tool")).await;
    assert_eq!(result.unwrap().stop, WorkModelStop::ToolUse);
    assert_eq!(seen, ["tool:open_tab"]);
}

#[tokio::test]
async fn error_auth_is_unauthorized() {
    let (result, seen) = call(&ClaudeAccountClient::new(cli()), request("error_auth")).await;
    assert_eq!(result.err(), Some(WorkModelError::Unauthorized));
    assert!(seen.is_empty());
}

#[tokio::test]
async fn usage_limit_maps_to_rate_limited() {
    let (result, _) = call(&ClaudeAccountClient::new(cli()), request("error_limit")).await;
    assert!(matches!(
        result.err(),
        Some(WorkModelError::RateLimited { .. })
    ));
}

#[tokio::test]
async fn garbage_is_protocol() {
    let (result, _) = call(&ClaudeAccountClient::new(cli()), request("garbage")).await;
    assert_eq!(result.err(), Some(WorkModelError::Protocol));
}

#[tokio::test]
async fn huge_output_is_bounded() {
    let (result, _) = call(&ClaudeAccountClient::new(cli()), request("huge")).await;
    assert_eq!(result.err(), Some(WorkModelError::Protocol));
}

#[tokio::test]
async fn hang_hits_idle_timeout() {
    let client = ClaudeAccountClient::new(cli())
        .with_timeouts(Duration::from_millis(500), Duration::from_secs(30));
    let started = std::time::Instant::now();
    let (result, _) = call(&client, request("hang")).await;
    assert_eq!(result.err(), Some(WorkModelError::Network));
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[tokio::test]
async fn streamed_progress_keeps_a_long_reply_alive() {
    // Partial events arrive every 200 ms for ~1.5 s; idle is 500 ms.
    let client = ClaudeAccountClient::new(cli())
        .with_timeouts(Duration::from_millis(500), Duration::from_secs(30));
    let (result, seen) = call(&client, request("slow_stream")).await;
    assert_eq!(result.unwrap().stop, WorkModelStop::EndTurn);
    assert_eq!(seen, ["text:fake reply"]);
}

#[tokio::test]
async fn below_min_version_is_missing_key() {
    let old = ClaudeCli {
        version: (2, 1, 100),
        ..cli()
    };
    let (result, _) = call(&ClaudeAccountClient::new(old), request("text")).await;
    assert_eq!(result.err(), Some(WorkModelError::MissingKey));
}

#[tokio::test]
async fn unknown_alias_is_bad_request() {
    let mut request = request("text");
    request.model.model = "claude-opus-5-5".into();
    let (result, _) = call(&ClaudeAccountClient::new(cli()), request).await;
    assert_eq!(result.err(), Some(WorkModelError::BadRequest));
}

#[tokio::test]
async fn child_env_has_no_anthropic_key() {
    // SAFETY: test-only; set before the child is spawned, read by nothing else.
    unsafe { std::env::set_var("ANTHROPIC_API_KEY", "sk-should-not-leak") };
    let (result, _) = call(&ClaudeAccountClient::new(cli()), request("echo_env")).await;
    let outcome = result.unwrap();
    let text = match &outcome.assistant[0] {
        WorkModelPart::Text(text) => text.clone(),
        _ => panic!("no text"),
    };
    assert!(text.contains("PATH"), "{text}");
    assert!(!text.contains("ANTHROPIC"), "{text}");
}

#[tokio::test]
async fn drop_kills_process_tree() {
    let dir = tempfile::tempdir().unwrap();
    let pidfile = dir.path().join("grandchild.pid");
    let mut request = request("spawn_child_and_hang");
    request.system.push(WorkModelSystemBlock {
        text: format!("FAKE_PIDFILE={}", pidfile.display()),
        cache: false,
    });
    let client = ClaudeAccountClient::new(cli());
    // Boxed so `drop` ends the future itself, not a pinned reference to it.
    let mut call = Box::pin(call(&client, request));
    let pid = loop {
        tokio::select! {
            _ = &mut call => panic!("the hanging call finished"),
            () = tokio::time::sleep(Duration::from_millis(50)) => {}
        }
        if let Some(pid) = std::fs::read_to_string(&pidfile)
            .ok()
            .and_then(|text| text.trim().parse::<u32>().ok())
        {
            break pid;
        }
    };
    assert!(alive(pid));
    drop(call);
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while alive(pid) && std::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(!alive(pid), "grandchild {pid} outlived the dropped call");
}

#[cfg(unix)]
fn alive(pid: u32) -> bool {
    // SAFETY: signal 0 only checks that the pid exists.
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

#[cfg(windows)]
fn alive(pid: u32) -> bool {
    let out = std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).contains(&pid.to_string())
}

/// Host check through the real, signed-in CLI on the person's plan:
/// `--ignored`, never in CI.
#[tokio::test]
#[ignore = "spends a little Claude plan usage"]
async fn real_cli_round_trip() {
    let (_, cli) = zephium_app::claude_account::probe().await;
    let client = ClaudeAccountClient::new(cli.expect("signed-in Claude Code"));
    let mut request = request("unused");
    request.system = vec![WorkModelSystemBlock {
        text: "You drive a browser for the person.".into(),
        cache: true,
    }];
    request.model.model = "haiku".into();
    request.tool_choice = WorkModelToolChoice::Required;
    let (result, seen) = call(&client, request).await;
    let outcome = result.unwrap();
    assert_eq!(outcome.stop, WorkModelStop::ToolUse);
    assert!(seen.contains(&"tool:open_tab".to_owned()), "{seen:?}");
}
