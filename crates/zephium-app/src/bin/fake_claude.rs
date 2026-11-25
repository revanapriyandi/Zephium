//! Test-only stand-in for the Claude Code CLI (`claude-fixture` feature).
//! The client clears the environment, so the behavior comes from the system
//! prompt file: `FAKE_MODE=<mode>` and, for process-tree checks,
//! `FAKE_PIDFILE=<path>`.
use std::io::{BufRead, Write};
use std::time::Duration;

use serde_json::json;

#[expect(
    clippy::zombie_processes,
    reason = "the process-tree check needs a descendant the client must end"
)]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["--version"] => return println!("2.1.331 (Claude Code)"),
        ["auth", "status"] => {
            return println!(
                r#"{{"loggedIn":true,"authMethod":"claude.ai","email":"fake@example.com"}}"#
            )
        }
        ["--fake-sleep"] => return std::thread::sleep(Duration::from_secs(60)),
        _ => {}
    }
    let prompt = args
        .iter()
        .position(|arg| arg == "--system-prompt-file")
        .and_then(|at| args.get(at + 1))
        .and_then(|path| std::fs::read_to_string(path).ok())
        .unwrap_or_default();
    let value = |key: &str| {
        prompt
            .lines()
            .find_map(|line| line.strip_prefix(key))
            .unwrap_or_default()
            .trim()
            .to_owned()
    };
    let mode = value("FAKE_MODE=");
    let mut input = String::new();
    let _ = std::io::stdin().lock().read_line(&mut input);
    let mut out = std::io::stdout().lock();
    let mut emit = |event: serde_json::Value| {
        let _ = writeln!(out, "{event}");
        let _ = out.flush();
    };
    emit(json!({"type": "system", "subtype": "init", "tools": ["StructuredOutput"]}));
    let usage = json!({"input_tokens": 3, "cache_read_input_tokens": 0,
        "cache_creation_input_tokens": 0, "output_tokens": 5});
    let success = |reply: serde_json::Value| {
        json!({"type": "result", "subtype": "success", "is_error": false,
            "structured_output": reply, "usage": usage, "total_cost_usd": 0.5})
    };
    match mode.as_str() {
        "text" => emit(success(json!({"text": "fake reply", "tool_calls": []}))),
        "tool" => emit(success(json!({"text": "", "tool_calls": [
            {"name": "open_tab", "arguments": {"url": "https://example.com"}}]}))),
        "echo_env" => {
            let names: Vec<String> = std::env::vars().map(|(name, _)| name).collect();
            emit(success(json!({"text": names.join(","), "tool_calls": []})));
        }
        "error_auth" => emit(json!({"type": "result", "subtype": "success",
            "is_error": true, "api_error_status": 401, "result": "Please run /login"})),
        "error_limit" => emit(json!({"type": "result", "subtype": "success",
            "is_error": true, "result": "Claude AI usage limit reached|4102444800"})),
        "garbage" => {
            let _ = writeln!(std::io::stdout(), "not json at all");
        }
        "huge" => {
            let line = "x".repeat(9 * 1024 * 1024);
            let _ = writeln!(std::io::stdout(), "{line}");
        }
        "hang" => std::thread::sleep(Duration::from_secs(60)),
        "slow_stream" => {
            for _ in 0..8 {
                std::thread::sleep(Duration::from_millis(200));
                emit(json!({"type": "stream_event", "event": {"type": "content_block_delta"}}));
            }
            emit(success(json!({"text": "fake reply", "tool_calls": []})));
        }
        "spawn_child_and_hang" => {
            let child = std::process::Command::new(std::env::current_exe().unwrap())
                .arg("--fake-sleep")
                .spawn()
                .unwrap();
            let _ = std::fs::write(value("FAKE_PIDFILE="), child.id().to_string());
            std::thread::sleep(Duration::from_secs(60));
        }
        _ => std::process::exit(2),
    }
}
