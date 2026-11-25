//! Finds the person's Claude Code CLI and asks it, through its own narrow
//! broker, which version it is and how it is signed in. Only a claude.ai
//! sign-in may run: any API-key login would spend an API balance.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;
use tokio::io::AsyncReadExt;
use zephium_mcp::StdioServer;

use crate::work_connections::cli::{locate, Cli};

/// Where the person's Claude Code stands, as Settings shows it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClaudeAccountState {
    NotInstalled,
    /// Found, but not an install this broker runs (a Windows batch shim).
    UnsupportedInstall,
    TooOld,
    SignedOut,
    /// Signed in, but with an API key or console login, not a Claude plan.
    ApiKeyLogin,
    SignedIn {
        account: Option<String>,
    },
}

/// A found, runnable Claude Code.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClaudeCli {
    pub program: PathBuf,
    pub version: (u32, u32, u32),
}

/// The oldest Claude Code that serves each model alias.
pub fn min_version(alias: &str) -> Option<(u32, u32, u32)> {
    Some(match alias {
        "opus" => (2, 1, 280),
        "sonnet" => (2, 1, 284),
        "haiku" => (2, 1, 293),
        "fable" => (2, 1, 321),
        _ => return None,
    })
}

/// The oldest Claude Code any alias runs on.
const MIN_ANY: (u32, u32, u32) = (2, 1, 280);
const PROBE_TIMEOUT: Duration = Duration::from_secs(8);
const PROBE_OUTPUT_BYTES: usize = 64 * 1024;

/// "2.1.331 (Claude Code)" → (2, 1, 331).
pub(crate) fn parse_version(out: &str) -> Option<(u32, u32, u32)> {
    let word = out.split_whitespace().next()?;
    let mut parts = word.split('.').map(|part| part.parse::<u32>().ok());
    let version = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(version)
}

/// `claude auth status` → state. Only a claude.ai login counts as signed in.
pub(crate) fn parse_auth(out: &str) -> ClaudeAccountState {
    let Ok(value) = serde_json::from_str::<Value>(out.trim()) else {
        return ClaudeAccountState::SignedOut;
    };
    if value["loggedIn"] != Value::Bool(true) {
        return ClaudeAccountState::SignedOut;
    }
    if value["authMethod"].as_str() != Some("claude.ai") {
        return ClaudeAccountState::ApiKeyLogin;
    }
    ClaudeAccountState::SignedIn {
        account: value["email"].as_str().map(str::to_owned),
    }
}

/// Batch shims re-parse arguments through cmd.exe; only real programs run.
pub(crate) fn supported_program(program: &Path) -> bool {
    if cfg!(windows) {
        program
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
    } else {
        true
    }
}

/// The first `claude.exe` in `dirs`: Anthropic's native installer, which
/// this broker runs, wins over an npm batch shim found earlier on PATH.
pub(crate) fn native_exe_in(dirs: &[PathBuf]) -> Option<PathBuf> {
    dirs.iter()
        .map(|dir| dir.join("claude.exe"))
        .find(|path| path.is_file())
}

/// Where a native `claude.exe` may be: PATH, then the installer's folder.
fn native_exe_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();
    if let Some(home) = std::env::var_os("USERPROFILE") {
        dirs.push(PathBuf::from(home).join(".local").join("bin"));
    }
    dirs
}

/// Variables the CLI needs to find itself and its own login, nothing else.
const ALLOWED_ENV: [&str; 22] = [
    "PATH",
    // A proxy the person's network needs. Never logged: it may hold credentials.
    "HTTPS_PROXY",
    "HTTP_PROXY",
    "NO_PROXY",
    "ALL_PROXY",
    "NODE_EXTRA_CA_CERTS",
    "HOME",
    "USER",
    "LOGNAME",
    "LANG",
    "TMPDIR",
    "XDG_CONFIG_HOME",
    "CLAUDE_CONFIG_DIR",
    "USERPROFILE",
    "HOMEDRIVE",
    "HOMEPATH",
    "APPDATA",
    "LOCALAPPDATA",
    "TEMP",
    "TMP",
    "SystemRoot",
    "SystemDrive",
];

/// The whole environment a Claude Code child gets, from `vars`: allowed
/// names only (never an API key or another provider's switch), the
/// program's own folder first on PATH, and no self-update mid-call.
pub(crate) fn child_env_from(
    program: &Path,
    vars: impl IntoIterator<Item = (String, String)>,
) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = Vec::new();
    for (name, value) in vars {
        let Some(allowed) = ALLOWED_ENV
            .iter()
            .find(|allowed| allowed.eq_ignore_ascii_case(&name))
        else {
            continue;
        };
        if !env.iter().any(|(known, _)| known == allowed) {
            env.push(((*allowed).to_owned(), value));
        }
    }
    let mut path: Vec<PathBuf> = program
        .parent()
        .map(Path::to_path_buf)
        .into_iter()
        .collect();
    if let Some((_, existing)) = env.iter().find(|(name, _)| name == "PATH") {
        path.extend(std::env::split_paths(&OsString::from(existing)));
    }
    env.retain(|(name, _)| name != "PATH");
    if let Ok(joined) = std::env::join_paths(path) {
        env.push(("PATH".into(), joined.to_string_lossy().into_owned()));
    }
    env.push(("DISABLE_AUTOUPDATER".into(), "1".into()));
    env
}

/// [`child_env_from`] over this process's environment.
pub fn child_env(program: &Path) -> Vec<(String, String)> {
    child_env_from(program, std::env::vars())
}

/// Runs `program args`, bounded in time and output, and returns all of its
/// stdout. Callers judge success by parsing it, so the exit status is never
/// reaped (dropping the process ends its whole group or job).
async fn capture(program: &Path, args: &[&str]) -> Option<String> {
    let server = StdioServer {
        program: program.to_path_buf(),
        args: args.iter().map(|arg| (*arg).to_owned()).collect(),
        env: child_env(program),
        cwd: Some(std::env::temp_dir()),
    };
    let (process, stdout, stdin) = server.spawn().ok()?;
    drop(stdin);
    let read = async move {
        let mut out = Vec::new();
        stdout
            .take(PROBE_OUTPUT_BYTES as u64 + 1)
            .read_to_end(&mut out)
            .await
            .ok()?;
        (out.len() <= PROBE_OUTPUT_BYTES).then_some(out)
    };
    let out = tokio::time::timeout(PROBE_TIMEOUT, read).await.ok()??;
    drop(process);
    Some(String::from_utf8_lossy(&out).into_owned())
}

/// How `program` is signed in now.
pub(crate) async fn auth_state(program: &Path) -> ClaudeAccountState {
    capture(program, &["auth", "status"])
        .await
        .map_or(ClaudeAccountState::SignedOut, |out| parse_auth(&out))
}

/// Finds Claude Code and asks how it is signed in. Never prompts, never
/// runs the legacy ambient CLI path.
pub async fn probe() -> (ClaudeAccountState, Option<ClaudeCli>) {
    let Some(program) = tokio::task::spawn_blocking(|| locate(Cli::Claude))
        .await
        .ok()
        .flatten()
    else {
        return (ClaudeAccountState::NotInstalled, None);
    };
    let program = if supported_program(&program) {
        program
    } else {
        match tokio::task::spawn_blocking(|| native_exe_in(&native_exe_dirs()))
            .await
            .ok()
            .flatten()
        {
            Some(native) => native,
            None => return (ClaudeAccountState::UnsupportedInstall, None),
        }
    };
    let Some(version) = capture(&program, &["--version"])
        .await
        .as_deref()
        .and_then(parse_version)
    else {
        return (ClaudeAccountState::NotInstalled, None);
    };
    if version < MIN_ANY {
        return (ClaudeAccountState::TooOld, None);
    }
    let state = auth_state(&program).await;
    let cli = matches!(state, ClaudeAccountState::SignedIn { .. })
        .then_some(ClaudeCli { program, version });
    (state, cli)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_versions() {
        assert_eq!(parse_version("2.1.331 (Claude Code)\n"), Some((2, 1, 331)));
        assert_eq!(parse_version("2.10.0"), Some((2, 10, 0)));
        assert_eq!(parse_version("Claude Code"), None);
        assert_eq!(parse_version("2.1"), None);
    }

    #[test]
    fn api_key_login_is_refused() {
        for method in ["api_key", "console", "apiKeyHelper", ""] {
            let out = format!(r#"{{"loggedIn":true,"authMethod":"{method}"}}"#);
            assert_eq!(
                parse_auth(&out),
                ClaudeAccountState::ApiKeyLogin,
                "{method}"
            );
        }
        assert_eq!(
            parse_auth(r#"{"loggedIn":true}"#),
            ClaudeAccountState::ApiKeyLogin
        );
    }

    #[test]
    fn claude_ai_login_is_signed_in() {
        assert_eq!(
            parse_auth(r#"{"loggedIn":true,"authMethod":"claude.ai","email":"a@b.c"}"#),
            ClaudeAccountState::SignedIn {
                account: Some("a@b.c".into())
            }
        );
        assert_eq!(
            parse_auth(r#"{"loggedIn":true,"authMethod":"claude.ai"}"#),
            ClaudeAccountState::SignedIn { account: None }
        );
    }

    #[test]
    fn signed_out_and_garbage() {
        assert_eq!(
            parse_auth(r#"{"loggedIn":false}"#),
            ClaudeAccountState::SignedOut
        );
        assert_eq!(parse_auth("x"), ClaudeAccountState::SignedOut);
        assert_eq!(parse_auth(""), ClaudeAccountState::SignedOut);
    }

    #[test]
    fn env_drops_anthropic_vars() {
        let program = std::env::temp_dir().join("bin").join("claude");
        let env = child_env_from(
            &program,
            [
                ("ANTHROPIC_API_KEY", "sk-secret"),
                ("ANTHROPIC_BASE_URL", "https://evil"),
                ("CLAUDE_CODE_USE_BEDROCK", "1"),
                ("PATH", "/usr/bin"),
                ("HOME", "/home/p"),
                ("RANDOM_THING", "x"),
            ]
            .map(|(k, v)| (k.to_owned(), v.to_owned())),
        );
        let names: Vec<&str> = env.iter().map(|(k, _)| k.as_str()).collect();
        assert!(!names.iter().any(|k| k.starts_with("ANTHROPIC_")));
        assert!(!names.contains(&"CLAUDE_CODE_USE_BEDROCK"));
        assert!(!names.contains(&"RANDOM_THING"));
        assert!(names.contains(&"HOME"));
        let path = &env.iter().find(|(k, _)| k == "PATH").unwrap().1;
        let first = std::env::split_paths(path).next().unwrap();
        assert_eq!(first, program.parent().unwrap());
    }

    #[test]
    fn env_keeps_proxy_settings() {
        let program = std::env::temp_dir().join("claude");
        let env = child_env_from(
            &program,
            [
                ("HTTPS_PROXY", "http://proxy:3128"),
                ("no_proxy", "localhost"),
                ("NODE_EXTRA_CA_CERTS", "/etc/ca.pem"),
            ]
            .map(|(k, v)| (k.to_owned(), v.to_owned())),
        );
        let names: Vec<&str> = env.iter().map(|(k, _)| k.as_str()).collect();
        for name in ["HTTPS_PROXY", "NO_PROXY", "NODE_EXTRA_CA_CERTS"] {
            assert!(names.contains(&name), "{name}");
        }
    }

    #[test]
    fn native_exe_wins_over_a_shim() {
        let shims = tempfile::tempdir().unwrap();
        let native = tempfile::tempdir().unwrap();
        std::fs::write(shims.path().join("claude.cmd"), "").unwrap();
        std::fs::write(native.path().join("claude.exe"), "").unwrap();
        let dirs = [shims.path().to_path_buf(), native.path().to_path_buf()];
        assert_eq!(native_exe_in(&dirs), Some(native.path().join("claude.exe")));
        assert_eq!(native_exe_in(&dirs[..1]), None);
    }

    #[test]
    fn windows_cmd_shim_unsupported() {
        assert!(supported_program(std::path::Path::new("C:/x/claude.exe")) || cfg!(unix));
        if cfg!(windows) {
            assert!(!supported_program(std::path::Path::new("C:/x/claude.cmd")));
            assert!(!supported_program(std::path::Path::new("C:/x/claude.bat")));
        }
        if cfg!(unix) {
            assert!(supported_program(std::path::Path::new(
                "/usr/local/bin/claude"
            )));
        }
    }

    /// Host check against the real CLI: `--ignored`, never in CI.
    #[tokio::test]
    #[ignore = "needs an installed, signed-in Claude Code"]
    async fn probe_real_cli() {
        let (state, cli) = probe().await;
        assert!(
            matches!(state, ClaudeAccountState::SignedIn { .. }),
            "{state:?}"
        );
        assert!(cli.unwrap().version >= MIN_ANY);
    }

    #[test]
    fn min_versions_table() {
        assert_eq!(min_version("opus"), Some((2, 1, 280)));
        assert_eq!(min_version("sonnet"), Some((2, 1, 284)));
        assert_eq!(min_version("haiku"), Some((2, 1, 293)));
        assert_eq!(min_version("fable"), Some((2, 1, 321)));
        assert_eq!(min_version("claude-opus-5-5"), None);
    }
}
