import "$styles/global.css";
import { afterEach, expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { preferences } from "$domain/preferences";
import AppearanceStage from "./AppearanceStage.svelte";

const native = vi.hoisted(() => ({
  settings: {} as Record<string, string>,
  set: vi.fn(),
}));
vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({
    settingGet: async (key: string) => native.settings[key] ?? null,
    settingSet: async (key: string, value: string) => {
      native.set(key, value);
      native.settings[key] = value;
      return { accepted: true, operation_id: null };
    },
  });
});

afterEach(() => {
  preferences.dispose();
  native.settings = {};
  native.set.mockClear();
});

const group = () =>
  document.querySelector<HTMLElement>('[role="radiogroup"][aria-label="Tab position"]')!;
const checked = () => group().querySelector('[aria-checked="true"]')?.textContent?.trim();

test("tab position starts in the sidebar and moves the tabs on top when chosen", async () => {
  await preferences.init();
  const screen = await render(AppearanceStage);
  await expect.poll(() => group()).not.toBeNull();
  expect(checked()).toBe("Sidebar");

  await screen.getByRole("radio", { name: "Top" }).click();
  await expect.poll(() => native.set.mock.calls.at(-1)).toEqual(["ui.tab-layout", "horizontal"]);
  await expect.poll(() => preferences.value("ui.tab-layout")).toBe("horizontal");
  expect(checked()).toBe("Top");

  await screen.getByRole("radio", { name: "Sidebar" }).click();
  await expect.poll(() => native.set.mock.calls.at(-1)).toEqual(["ui.tab-layout", "vertical"]);
});

test("a stored value this build does not know shows the tabs in the sidebar", async () => {
  native.settings = { "ui.tab-layout": "diagonal" };
  await preferences.init();
  await render(AppearanceStage);
  await expect.poll(() => group()).not.toBeNull();
  expect(checked()).toBe("Sidebar");
});
