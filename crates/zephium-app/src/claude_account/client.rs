//! One model call = one `claude -p` child: argv only (no shell), the cleared
//! environment from `detect`, a fresh empty working folder, every Claude
//! Code tool, MCP server, setting source and slash command off. Dropping the
//! call ends the child's whole process group (macOS) or job (Windows).

use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use zephium_core::work::model::{
    WorkModelClient, WorkModelError, WorkModelEvent, WorkModelFuture, WorkModelOutcome,
    WorkModelPart, WorkModelProvider, WorkModelRequest,
};
use zephium_mcp::StdioServer;

use super::detect::{auth_state, child_env, min_version, ClaudeAccountState, ClaudeCli};
use super::wire::{parse_reply, stdin_lines, system_prompt, REPLY_SCHEMA};

const IDLE_TIMEOUT: Duration = Duration::from_secs(120);
const TOTAL_TIMEOUT: Duration = Duration::from_secs(600);
/// How long one claude.ai sign-in check is trusted before asking again.
const AUTH_FRESH: Duration = Duration::from_secs(60);
const MAX_OUTPUT_BYTES: usize = 32 * 1024 * 1024;
/// StructuredOutput takes two turns; one more absorbs a stray native call.
const MAX_TURNS: &str = "3";

/// The exact arguments of one call. `--setting-sources ""` keeps the
/// person's settings, hooks and plugins out; `--tools ""` leaves only the
/// StructuredOutput reply tool.
pub fn argv(alias: &str, system_prompt_file: &Path) -> Vec<String> {
    [
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        // Partial events stream while the reply is written, so the idle
        // limit measures a stalled CLI, not a long answer.
        "--include-partial-messages",
        "--verbose",
        "--tools",
        "",
        "--strict-mcp-config",
        "--setting-sources",
        "",
        "--disable-slash-commands",
        "--no-session-persistence",
        "--max-turns",
        MAX_TURNS,
        "--model",
        alias,
        "--json-schema",
        REPLY_SCHEMA,
        "--system-prompt-file",
    ]
    .into_iter()
    .map(str::to_owned)
    .chain([system_prompt_file.to_string_lossy().into_owned()])
    .collect()
}

/// Removes a call's folder once the ended child lets go of it: on Windows a
/// process that is being terminated can still hold its working folder, and
/// the folder holds the system prompt. Tries for a few seconds, off the
/// async runtime.
fn remove_after_exit(folder: tempfile::TempDir) {
    let path = folder.keep();
    tokio::task::spawn_blocking(move || {
        for _ in 0..50 {
            if std::fs::remove_dir_all(&path).is_ok() || !path.exists() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    });
}

/// A model client on the person's Claude subscription.
pub struct ClaudeAccountClient {
    cli: ClaudeCli,
    idle: Duration,
    total: Duration,
}

impl ClaudeAccountClient {
    pub fn new(cli: ClaudeCli) -> Self {
        Self {
            cli,
            idle: IDLE_TIMEOUT,
            total: TOTAL_TIMEOUT,
        }
    }

    /// Shorter limits, for tests.
    #[doc(hidden)]
    pub fn with_timeouts(mut self, idle: Duration, total: Duration) -> Self {
        self.idle = idle;
        self.total = total;
        self
    }

    /// Refuses unless the CLI is still signed in with a Claude plan, so an
    /// API-key login can never spend a balance. Trusted for a minute.
    async fn ensure_signed_in(&self) -> Result<(), WorkModelError> {
        static CHECKED: Mutex<Option<(std::path::PathBuf, Instant)>> = Mutex::new(None);
        let fresh = CHECKED.lock().ok().is_some_and(|checked| {
            checked.as_ref().is_some_and(|(program, at)| {
                *program == self.cli.program && at.elapsed() < AUTH_FRESH
            })
        });
        if fresh {
            return Ok(());
        }
        match auth_state(&self.cli.program).await {
            ClaudeAccountState::SignedIn { .. } => {
                if let Ok(mut checked) = CHECKED.lock() {
                    *checked = Some((self.cli.program.clone(), Instant::now()));
                }
                Ok(())
            }
            _ => Err(WorkModelError::Unauthorized),
        }
    }

    async fn run(&self, request: &WorkModelRequest) -> Result<Value, WorkModelError> {
        let lines = stdin_lines(request)?;
        let folder = tempfile::tempdir().map_err(|_| WorkModelError::Network)?;
        let prompt_file = folder.path().join("system-prompt.txt");
        std::fs::write(&prompt_file, system_prompt(request))
            .map_err(|_| WorkModelError::Network)?;
        let server = StdioServer {
            program: self.cli.program.clone(),
            args: argv(&request.model.model, &prompt_file),
            env: child_env(&self.cli.program),
            cwd: Some(folder.path().to_path_buf()),
        };
        let (process, stdout, mut stdin) = server.spawn().map_err(|_| WorkModelError::Network)?;
        let writer = tokio::spawn(async move {
            for line in lines {
                if stdin.write_all(line.as_bytes()).await.is_err()
                    || stdin.write_all(b"\n").await.is_err()
                {
                    return;
                }
            }
            let _ = stdin.shutdown().await;
        });
        let mut reader = BufReader::new(stdout).lines();
        let mut read = 0usize;
        let result = loop {
            let line = tokio::time::timeout(self.idle, reader.next_line())
                .await
                .map_err(|_| WorkModelError::Network)?
                .map_err(|_| WorkModelError::Protocol)?
                .ok_or(WorkModelError::Protocol)?;
            read = read.saturating_add(line.len());
            if read > MAX_OUTPUT_BYTES {
                return Err(WorkModelError::Protocol);
            }
            let Ok(event) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if event["type"] == "result" {
                break event;
            }
        };
        writer.abort();
        drop(process);
        remove_after_exit(folder);
        Ok(result)
    }

    async fn call_once(
        &self,
        request: WorkModelRequest,
        events: &(dyn Fn(WorkModelEvent) + Send + Sync),
    ) -> Result<WorkModelOutcome, WorkModelError> {
        if request.model.provider != WorkModelProvider::ClaudeAccount {
            return Err(WorkModelError::BadRequest);
        }
        let minimum = min_version(&request.model.model).ok_or(WorkModelError::BadRequest)?;
        if self.cli.version < minimum {
            return Err(WorkModelError::MissingKey);
        }
        self.ensure_signed_in().await?;
        let run = tokio::time::timeout(self.total, self.run(&request));
        let result = match request.image_fence.clone() {
            Some(fence) => tokio::select! {
                biased;
                () = fence.revoked() => return Err(WorkModelError::Cancelled),
                result = run => result,
            },
            None => run.await,
        }
        .map_err(|_| WorkModelError::Network)??;
        let outcome = parse_reply(&request, &result)?;
        for part in &outcome.assistant {
            match part {
                WorkModelPart::Text(text) => events(WorkModelEvent::Text(text.clone())),
                WorkModelPart::ToolCall(call) => events(WorkModelEvent::ToolCall(call.clone())),
                WorkModelPart::Image { .. } | WorkModelPart::Replay(_) => {}
            }
        }
        Ok(outcome)
    }
}

impl WorkModelClient for ClaudeAccountClient {
    fn call<'a>(
        &'a self,
        request: WorkModelRequest,
        events: &'a (dyn Fn(WorkModelEvent) + Send + Sync),
    ) -> WorkModelFuture<'a> {
        Box::pin(self.call_once(request, events))
    }
}
