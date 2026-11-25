//! Model calls through the person's own signed-in Claude Code CLI, on their
//! Claude subscription. Zephium never holds the account's token: the CLI does.
//! Every call runs with Claude Code's own tools, MCP servers, settings, hooks
//! and plugins off; it only returns text and tool calls, which Zephium's loop
//! admits and runs like any other model's. The legacy ambient CLI gate stays
//! closed; this module is its own narrow broker.

mod client;
mod detect;
mod wire;

pub use client::{argv, ClaudeAccountClient};

pub use detect::{child_env, min_version, probe, ClaudeAccountState, ClaudeCli};

use zephium_core::work::model::{
    WorkModelEntry, WorkModelPrice, WorkModelProvider, WorkModelRef, WorkModelRole,
    WorkModelSupports, WorkModelWire,
};

/// The models a Claude plan runs, by Claude Code's own aliases. Free in
/// Work's accounting: only the plan's usage is spent. The context bound is
/// the CLI route's, below the models' own windows.
pub fn entries() -> Vec<WorkModelEntry> {
    use WorkModelRole as R;
    [
        ("opus", "Claude Opus (your plan)", &[R::Lead, R::Page][..]),
        (
            "sonnet",
            "Claude Sonnet (your plan)",
            &[R::Lead, R::Page][..],
        ),
        (
            "haiku",
            "Claude Haiku (your plan)",
            &[R::Page, R::Light][..],
        ),
        ("fable", "Claude Fable (your plan)", &[R::Lead, R::Page][..]),
    ]
    .into_iter()
    .map(|(alias, name, roles)| WorkModelEntry {
        id: format!("claude_account/{alias}"),
        model: WorkModelRef {
            provider: WorkModelProvider::ClaudeAccount,
            wire: WorkModelWire::AnthropicMessages,
            model: alias.to_owned(),
        },
        display_name: name.to_owned(),
        roles: roles.to_vec(),
        recommended: alias == "sonnet",
        context_window: 200_000,
        max_output: 32_000,
        supports: WorkModelSupports {
            tools: true,
            vision: true,
            prompt_cache: false,
            reasoning: false,
            native_search: false,
        },
        price: Some(WorkModelPrice {
            input: 0,
            cached_input: 0,
            output: 0,
        }),
    })
    .collect()
}
