<script lang="ts">
  import type { WorkClaudeAccountStateV1 } from "$shared/ipc/bindings";
  import type { ModelsSession } from "$domain/ai";
  import ProviderMark from "$shared/ui/ProviderMark";
  import Switch from "$shared/ui/Switch";
  import * as m from "$shared/i18n/messages";

  let { session }: { session: ModelsSession } = $props();

  const account = $derived(session.models?.claude_account ?? null);
  const signedIn = $derived(account?.state === "signed_in");
  let failed = $state(false);

  const statusText: Record<WorkClaudeAccountStateV1, () => string> = {
    unknown: m.ai_claude_account_unknown,
    not_installed: m.ai_claude_account_not_installed,
    unsupported_install: m.ai_claude_account_unsupported,
    too_old: m.ai_claude_account_too_old,
    signed_out: m.ai_claude_account_signed_out,
    api_key_login: m.ai_claude_account_api_key,
    signed_in: m.ai_claude_account_signed_in_plain,
  };
  const status = $derived.by(() => {
    if (!account) return m.ai_claude_account_unknown();
    if (account.state === "signed_in" && account.account)
      return m.ai_claude_account_signed_in({ account: account.account });
    return statusText[account.state]();
  });

  // Turning it on looks at Claude Code again, so the switch stays usable
  // while it is not ready yet; Rust runs nothing until it is signed in.
  async function toggle(enabled: boolean) {
    failed = !(await session.setClaudeAccount(enabled));
  }
</script>

<div class="ai-row">
  <div class="head">
    <span class="tile" aria-hidden="true"><ProviderMark provider="anthropic" size={20} /></span>
    <div class="copy">
      <h3>{m.ai_claude_account()}</h3>
      <p class="status" data-ready={signedIn}>{status}</p>
    </div>
    <Switch
      label={m.ai_claude_account()}
      labelHidden
      checked={!!account?.enabled}
      disabled={!!session.busy}
      onchange={(enabled) => void toggle(enabled)}
    />
  </div>
  <p class="message">{m.ai_claude_account_desc()}</p>
  {#if failed}<p class="message" data-tone="error" role="alert">
      {m.ai_claude_account_failed()}
    </p>{/if}
</div>

<style>
  /* Metrics match AiProviderRow so the keys group reads as one list. */
  .ai-row {
    position: relative;
    box-sizing: border-box;
    padding: 12px 18px 12px 14px;
  }

  :global(.ai-row) + .ai-row::before {
    content: "";
    position: absolute;
    inset-inline: 58px 0;
    top: 0;
    height: 1px;
    background: var(--color-border);
  }

  .head {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: calc(var(--row-page) - 24px);
  }

  .tile {
    display: grid;
    flex: none;
    inline-size: 32px;
    block-size: 32px;
    border-radius: var(--radius-control);
    background: var(--color-fill);
    color: var(--color-text);
    place-items: center;
  }

  .copy {
    flex: 1;
    min-width: 0;
  }

  h3 {
    margin: 0;
    color: var(--color-text);
    font-size: var(--text-page-title);
    font-weight: 500;
    line-height: 19px;
    letter-spacing: -0.008em;
  }

  p {
    margin: 2px 0 0;
    color: var(--color-muted);
    font-size: var(--text-label);
    line-height: 1.5;
  }

  .status[data-ready="true"] {
    color: var(--color-text);
  }

  .message {
    margin: 8px 0 0 44px;
  }

  .message[data-tone="error"] {
    color: var(--color-danger);
  }
</style>
