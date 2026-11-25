import "$styles/global.css";
import { afterEach, expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { page, userEvent } from "vitest/browser";
import type { SidebarNodeView } from "$shared/ipc/bindings";
import { emitNativeEvent } from "$shared/testing/native-events";
import { tabFixture, revision } from "$shared/testing/fixtures";
import { tabs } from "$domain/tabs";
import { surface } from "$domain/surface";
import { preferences } from "$domain/preferences";
import * as sidebar from "$session/sidebar-mode.svelte";
import Shell from "../Shell.svelte";

let screen: Awaited<ReturnType<typeof render>> | null = null;

const native = vi.hoisted(() => ({
  settings: {} as Record<string, string>,
  activate: vi.fn(async (_id: string) => ({ accepted: true, operation_id: null })),
  close: vi.fn(async (_id: string) => ({ accepted: true, operation_id: null })),
}));

vi.mock("$shared/platform", () => ({ IS_MAC: true, IS_WINDOWS: false }));
vi.mock("../WorkWorkspace.svelte", async () => await import("./StageStub.svelte"));
vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({
    pagePermissionCardHost: async () => {},
    tabsBootstrap: async () => {},
    tabsActivate: native.activate,
    tabsClose: native.close,
    sidebarSetHidden: async () => {},
    sidebarPeek: async () => {},
    sidebarSetWidth: async () => {},
    sidebarResize: async () => false,
    sidebarResizeGuide: async () => true,
    settingGet: async (key: string) => native.settings[key] ?? null,
    settingSet: async () => ({ accepted: true, operation_id: null }),
  });
});

afterEach(() => {
  screen?.unmount();
  screen = null;
  sidebar.adoptMode("default");
  surface.dispose();
  tabs.dispose();
  preferences.dispose();
  native.settings = {};
  vi.clearAllMocks();
});

let projection = 60;

async function open(layout: string | null) {
  await page.viewport(1000, 640);
  if (layout !== null) native.settings = { "ui.tab-layout": layout };
  await preferences.init();
  await surface.init();
  await tabs.init();
  const all = ["pin", "a", "b", "c"].map((id) =>
    tabFixture({ id, title: `Tab ${id}`, url: `https://${id}.example/` }),
  );
  const nodes: SidebarNodeView[] = all.map((tab) => ({
    id: tab.id,
    parent_id: null,
    section: tab.id === "pin" ? "pinned" : "today",
    kind: { type: "tab", tab_id: tab.id },
  }));
  emitNativeEvent("itemsChanged", {
    projection_revision: revision((projection += 1)),
    profile: null,
    spaces: [],
    active_space_id: null,
    nodes,
    tabs: all,
    active: "b",
    split_group: null,
  });
  screen = await render(Shell);
  return screen;
}

const strip = () =>
  screen!.container.querySelector<HTMLElement>('nav[data-top-tabs][aria-label="Tab strip"]');
const rows = (id: string) => screen!.container.querySelectorAll(`[data-zephium-tab-id="${id}"]`);

test("tabs on top stand in one strip over the page, each with its single row", async () => {
  await open("horizontal");
  await expect.poll(() => strip()).not.toBeNull();
  for (const id of ["a", "b", "c"]) {
    expect(rows(id)).toHaveLength(1);
    expect(strip()!.contains(rows(id)[0]!)).toBe(true);
    expect(rows(id)[0]!.querySelector("[data-zephium-tab-label]")?.textContent).toBe(`Tab ${id}`);
  }
  // Pinned tabs stay in the column.
  expect(rows("pin")).toHaveLength(1);
  expect(strip()!.contains(rows("pin")[0]!)).toBe(false);
  expect(strip()!.querySelector('[aria-current="page"]')?.getAttribute("aria-label")).toBe("Tab b");
  // The strip is the stage's first band: the page lies beneath it.
  expect(strip()!.parentElement?.classList.contains("stage-column")).toBe(true);
  expect(strip()!.getBoundingClientRect().height).toBe(32);
});

test("the strip opens, closes and walks its tabs from the keyboard", async () => {
  const shell = await open("horizontal");
  await expect.poll(() => strip()).not.toBeNull();
  await shell.getByRole("button", { name: "Tab c", exact: true }).click();
  await expect.poll(() => native.activate.mock.calls.at(-1)).toEqual(["c"]);
  await shell.getByRole("button", { name: "Close Tab a" }).click();
  await expect.poll(() => native.close.mock.calls.at(-1)).toEqual(["a"]);

  const first = strip()!.querySelector<HTMLElement>(
    '[data-zephium-tab-id="a"] [data-top-tab-open]',
  )!;
  first.focus();
  await userEvent.keyboard("{ArrowRight}");
  expect(document.activeElement?.getAttribute("aria-label")).toBe("Tab b");
  await userEvent.keyboard("{End}");
  expect(document.activeElement?.getAttribute("aria-label")).toBe("Tab c");
  await userEvent.keyboard("{Home}");
  expect(document.activeElement?.getAttribute("aria-label")).toBe("Tab a");
});

for (const [name, layout] of [
  ["by default", null],
  ["for a stored value this build does not know", "diagonal"],
] as const) {
  test(`tabs stay in the sidebar ${name}`, async () => {
    await open(layout);
    await expect.poll(() => rows("b").length).toBe(1);
    expect(strip()).toBeNull();
    expect(rows("b")[0]!.closest("aside")).not.toBeNull();
  });
}

test("a hidden sidebar keeps its tabs in its card", async () => {
  await open("horizontal");
  await expect.poll(() => strip()).not.toBeNull();
  sidebar.adoptMode("hidden");
  await expect.poll(() => strip()).toBeNull();
  await expect.poll(() => rows("b").length).toBe(1);
});

test("the compact rail leaves today's tabs to the strip", async () => {
  await open("horizontal");
  sidebar.setMode("compact");
  await expect.poll(() => strip()).not.toBeNull();
  for (const id of ["a", "b", "c"]) {
    await expect.poll(() => rows(id).length).toBe(1);
    expect(strip()!.contains(rows(id)[0]!)).toBe(true);
  }
});
