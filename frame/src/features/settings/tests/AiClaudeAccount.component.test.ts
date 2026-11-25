import "$styles/global.css";
import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { page, userEvent } from "vitest/browser";
import type { WorkLocalServerV1, WorkModelsV1 } from "$shared/ipc/bindings";
import { PROFILE, models } from "$shared/testing/work-models";
import AiPage from "../components/sections/AiPage.svelte";

vi.mock("$domain/tabs", () => ({
  tabs: { profile: () => ({ id: "00000000000000000000000001", kind: "regular" }) },
}));
const native = vi.hoisted(() => ({
  read: vi.fn(),
  claude: vi.fn(),
}));
vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({
    workModels: native.read,
    workSetClaudeAccount: native.claude,
    workLocalServers: async () => [] as WorkLocalServerV1[],
    accountGet: vi.fn(),
    voiceStatus: async () => ({
      version: 1,
      engines: ["on_device"],
      chosen: null,
      effective: "on_device",
      language: null,
      languages: ["en-US"],
      on_device: "en_US",
      microphone: "granted",
    }),
    workDecisionPreference: async () => ({
      version: 1,
      profile: PROFILE,
      choice: "recommended",
      effective: "standard",
      typesafe_key_present: false,
      error: null,
    }),
  });
});

function account(
  state: WorkModelsV1["claude_account"]["state"],
  enabled = false,
  email: string | null = null,
): WorkModelsV1 {
  return models({ claudeAccount: { enabled, state, account: email } });
}

const toggle = () => page.getByRole("switch", { name: "Use my Claude plan" });

test("each Claude Code state explains itself and the switch stays usable to check again", async () => {
  await page.viewport(1100, 1400);
  const cases: [WorkModelsV1["claude_account"]["state"], string][] = [
    ["not_installed", "Install Claude Code with Anthropic’s installer."],
    [
      "unsupported_install",
      "This Claude Code install isn’t supported. Use Anthropic’s native installer.",
    ],
    ["too_old", "Update Claude Code: run claude update."],
    ["signed_out", "Sign in: run claude auth login in a terminal."],
    [
      "api_key_login",
      "Claude Code is signed in with an API key. Sign in with your Claude account instead.",
    ],
  ];
  for (const [state, text] of cases) {
    native.read.mockResolvedValue(account(state));
    const screen = await render(AiPage);
    await expect.element(screen.getByText(text)).toBeVisible();
    await expect.element(toggle()).toBeEnabled();
    await screen.unmount();
  }
  native.read.mockResolvedValue(account("signed_out"));
  native.claude.mockResolvedValue(account("signed_in", true));
  const screen = await render(AiPage);
  await toggle().click();
  expect(native.claude).toHaveBeenCalledWith(PROFILE, true);
  await expect.element(toggle()).toBeChecked();
  await screen.unmount();
});

test("turning it on sends the choice to Rust and says the plan is never charged", async () => {
  await page.viewport(1100, 1400);
  native.read.mockResolvedValue(account("signed_in", false, "me@example.com"));
  native.claude.mockResolvedValue(account("signed_in", true, "me@example.com"));
  const screen = await render(AiPage);
  await expect.element(screen.getByText("Signed in as me@example.com")).toBeVisible();
  await expect
    .element(screen.getByText(/keep extra usage off on claude\.ai to never be charged/u))
    .toBeVisible();
  await expect.element(toggle()).toBeEnabled();
  await toggle().click();
  expect(native.claude).toHaveBeenCalledWith(PROFILE, true);
  await expect.element(toggle()).toBeChecked();
});

test("the switch works from the keyboard and a refusal is announced", async () => {
  await page.viewport(1100, 1400);
  native.read.mockResolvedValue(account("signed_in", true));
  native.claude.mockResolvedValue({ ...account("signed_in", true), fault: "keychain" });
  const screen = await render(AiPage);
  await expect.element(toggle()).toBeChecked();
  (toggle().element() as HTMLElement).focus();
  await userEvent.keyboard(" ");
  expect(native.claude).toHaveBeenCalledWith(PROFILE, false);
  await expect
    .element(screen.getByRole("alert"))
    .toHaveTextContent("Couldn’t change this. Try again.");
});
