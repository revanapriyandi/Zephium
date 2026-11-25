<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import { Add01Icon, Cancel01Icon } from "@hugeicons/core-free-icons";
  import FavIcon from "$shared/ui/FavIcon";
  import Icon from "$shared/ui/Icon";
  import { favicons } from "$domain/favicons";
  import { tabs } from "$domain/tabs";
  import { IS_WINDOWS } from "$shared/platform";
  import type { SidebarEntry } from "../lib/sidebar-model";
  import { tabFallbackIcon } from "../lib/tab-icon";
  import { closeOnMiddleClick, openOnBackgroundMiddleClick } from "../lib/middle-click";

  let {
    entries,
    onSelect,
  }: {
    /** Today's entries: with tabs on top, these rows are the only ones. */
    entries: SidebarEntry[];
    onSelect: (id: string) => void;
  } = $props();

  // ponytail: one flat strip; folders and split grouping keep their shape in
  // the sidebar only, and reordering by drag stays a sidebar gesture.
  const list = $derived(entries.flatMap((entry) => (entry.kind === "tab" ? [entry.tab] : [])));
  const backgroundClick = openOnBackgroundMiddleClick(tabs.open);

  // Arrow keys walk the strip in reading order; Home and End reach its ends.
  function walk(event: KeyboardEvent) {
    if (!(event.target instanceof HTMLElement)) return;
    const strip = event.currentTarget as HTMLElement;
    const buttons = [...strip.querySelectorAll<HTMLElement>("[data-top-tab-open]")];
    const at = buttons.indexOf(event.target);
    if (at === -1) return;
    const rtl = getComputedStyle(strip).direction === "rtl";
    const forward = rtl ? "ArrowLeft" : "ArrowRight";
    const back = rtl ? "ArrowRight" : "ArrowLeft";
    const next =
      event.key === forward
        ? Math.min(at + 1, buttons.length - 1)
        : event.key === back
          ? Math.max(at - 1, 0)
          : event.key === "Home"
            ? 0
            : event.key === "End"
              ? buttons.length - 1
              : null;
    if (next === null) return;
    event.preventDefault();
    buttons[next]?.focus();
  }
</script>

<!-- The strip's open ground moves the window, as the rest of the gutter does. -->
<nav
  class="top-tabs"
  data-top-tabs
  aria-label={m.top_tabs_label()}
  data-tauri-drag-region
  data-caption={IS_WINDOWS || undefined}
>
  <ul
    class="top-tab-list"
    data-tauri-drag-region
    data-tab-list-background
    onkeydown={walk}
    {...backgroundClick}
  >
    {#each list as tab (tab.id)}
      {@const active = tab.id === tabs.activeId()}
      <li
        class="top-tab"
        data-zephium-tab-id={tab.id}
        data-zephium-tab-url={tab.url ?? ""}
        data-zephium-projection-revision={tab.projection_revision}
        data-selected={active}
      >
        <button
          type="button"
          class="top-tab-open"
          data-top-tab-open
          aria-current={active ? "page" : undefined}
          aria-label={tab.title || m.untitled_tab()}
          title={tab.title || m.untitled_tab()}
          onclick={() => onSelect(tab.id)}
          oncontextmenu={(event) => {
            event.preventDefault();
            tabs.openTabMenu(tab.id, event.clientX, event.clientY);
          }}
          {...closeOnMiddleClick(() => tabs.close(tab.id))}
        >
          <FavIcon
            image={favicons.image(tab.icon)}
            tone={favicons.tone(tab.icon)}
            loading={tab.loading}
            size={16}
            lit={active}
            fallback={tabFallbackIcon(tab.content)}
            notched={!!tab.audio}
          />
          <span data-zephium-tab-label class="top-tab-label">{tab.title}</span>
        </button>
        <button
          type="button"
          class="top-tab-close"
          aria-label={m.close_named_tab({ title: tab.title || m.untitled_tab() })}
          title={m.close_tab()}
          onclick={() => tabs.close(tab.id)}
        >
          <Icon icon={Cancel01Icon} size={12} />
        </button>
      </li>
    {/each}
  </ul>
  <button
    type="button"
    class="top-tab-new"
    aria-label={m.new_tab()}
    title={m.new_tab()}
    onclick={tabs.open}
  >
    <Icon icon={Add01Icon} size={16} />
  </button>
</nav>

<style>
  /* 32px of strip and 8px under it: `zephium_core::layout::TAB_BAND`, the
     band native takes from the top of the page. */
  .top-tabs {
    display: flex;
    flex: none;
    align-items: center;
    gap: 4px;
    height: 32px;
    margin-block-end: 8px;
    padding-inline-start: 8px;
    min-width: 0;
  }

  /* The window's controls come down over the strip's end on Windows. */
  .top-tabs[data-caption] {
    padding-inline-end: 138px;
  }

  .top-tab-list {
    display: flex;
    flex: 0 1 auto;
    gap: 4px;
    align-items: center;
    height: 100%;
    min-width: 0;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .top-tab {
    position: relative;
    display: flex;
    flex: 0 1 200px;
    align-items: center;
    min-width: 56px;
    height: 100%;
    border-radius: var(--radius-row);
    color: var(--color-label-secondary);
    font-size: var(--sidebar-row-text);
    font-weight: var(--sidebar-row-weight);
    transition:
      background-color var(--motion-fast) var(--ease-out),
      color var(--motion-base) var(--ease-out);
  }

  .top-tab:not([data-selected="true"]):hover {
    background: var(--row-hover);
    color: var(--color-text);
  }

  .top-tab[data-selected="true"] {
    background: var(--row-active);
    box-shadow: var(--row-rim);
    color: var(--color-row-current, var(--color-text));
    font-weight: var(--sidebar-row-weight-current);
  }

  .top-tab-open {
    display: flex;
    flex: 1;
    align-items: center;
    align-self: stretch;
    gap: 8px;
    min-width: 0;
    padding-inline: 10px 4px;
    border: 0;
    border-radius: inherit;
    background: transparent;
    color: inherit;
    font: inherit;
    text-align: start;
    cursor: default;
  }

  .top-tab-open:focus-visible {
    outline-offset: -2px;
  }

  .top-tab-label {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    mask-image: var(--mask-fade-end);
  }

  .top-tab-label:dir(rtl) {
    mask-image: var(--mask-fade-end-rtl);
  }

  .top-tab-close,
  .top-tab-new {
    display: flex;
    flex: none;
    align-items: center;
    justify-content: center;
    border: 0;
    border-radius: var(--radius-inset);
    background: transparent;
    color: var(--color-faint);
    cursor: default;
    transition:
      opacity var(--motion-fast) var(--ease-out),
      background-color var(--motion-fast) var(--ease-out),
      color var(--motion-fast) var(--ease-out);
  }

  .top-tab-close {
    width: 21px;
    height: 21px;
    margin-inline-end: 5px;
    opacity: 0;
  }

  .top-tab-new {
    width: 28px;
    height: 28px;
  }

  .top-tab-close:hover,
  .top-tab-new:hover {
    background: var(--row-pressed);
    color: var(--color-text);
  }

  :global([data-close-buttons="always"]) .top-tab-close {
    opacity: 1;
  }

  .top-tab:is(:hover, :focus-within, [data-selected="true"]) .top-tab-close {
    opacity: 1;
  }

  @media (prefers-reduced-motion: reduce) {
    .top-tab,
    .top-tab-close,
    .top-tab-new {
      transition: none;
    }
  }

  @media (forced-colors: active) {
    .top-tab[data-selected="true"] {
      outline: 2px solid Highlight;
      outline-offset: -2px;
    }

    .top-tab-label {
      mask-image: none;
    }
  }
</style>
