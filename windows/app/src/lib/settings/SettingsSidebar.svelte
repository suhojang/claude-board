<script lang="ts">
  // 왼쪽 — 돌아가기 단추, 검색 칸, 묶음 여덟 개(mac 설정 화면의 sidebar). 검색어가 있으면 맞는 항목이 없는 묶음을 흐리게 한다.
  import Icon, { type IconName } from "$lib/board/Icon.svelte";
  import type { SettingsSectionId } from "$lib/types";
  import { SETTINGS_ITEMS } from "./items";
  import { SettingsScreen, closeSettings, goSection } from "./screen.svelte";
  import { matchedSections } from "./search";
  import { SECTION_ORDER, SETTINGS_SECTIONS } from "./sections";

  const ICONS: Record<SettingsSectionId, IconName> = {
    general: "wrench",
    window: "expand",
    notify: "bell",
    terminal: "terminal-outline",
    board: "panel-left",
    account: "person",
    shortcuts: "hand",
    update: "arrow-down-circle",
  };

  let searchEl: HTMLInputElement | undefined = $state();
  let searching = $state(false);

  const hasQuery = $derived(SettingsScreen.query.trim() !== "");
  const matched = $derived(new Set(matchedSections(SETTINGS_ITEMS, SettingsScreen.query, SECTION_ORDER)));

  // 열 때·다시 열 때 검색 칸에 포커스 — 터미널(xterm)이 가진 포커스를 가져온다
  $effect(() => {
    void SettingsScreen.focusSearch;
    requestAnimationFrame(() => searchEl?.focus());
  });
</script>

<nav class="sidebar" data-testid="settings-sidebar" aria-label="설정 묶음">
  <button class="back" type="button" onclick={() => closeSettings()} data-testid="settings-back">
    <Icon name="chevron-left" size={10} />
    <span class="backText">터미널로 돌아가기</span>
    <kbd>esc</kbd>
  </button>

  <div class="search" class:focus={searching} role="search">
    <input
      bind:this={searchEl}
      bind:value={SettingsScreen.query}
      onfocus={() => (searching = true)}
      onblur={() => (searching = false)}
      type="text"
      placeholder="설정 검색"
      aria-label="설정 검색"
      autocomplete="off"
      spellcheck="false"
      data-testid="settings-search"
    />
    {#if hasQuery}
      <button class="clear" type="button" aria-label="검색어 지우기" onclick={() => ((SettingsScreen.query = ""), searchEl?.focus())}>
        <Icon name="xmark-circle" size={12} />
      </button>
    {/if}
  </div>

  <ul class="sections" role="list">
    {#each SETTINGS_SECTIONS as section (section.id)}
      {@const on = !hasQuery && SettingsScreen.section === section.id}
      <li>
        <button
          class="section"
          class:on
          class:dim={hasQuery && !matched.has(section.id)}
          type="button"
          aria-current={on ? "page" : undefined}
          data-testid={`settings-section-${section.id}`}
          onclick={() => goSection(section.id)}
        >
          <span class="icon"><Icon name={ICONS[section.id]} size={13} /></span>
          <span>{section.title}</span>
        </button>
      </li>
    {/each}
  </ul>
</nav>

<style>
  .sidebar {
    flex: 0 0 200px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px;
    background: var(--term-bar);
    border-right: 1px solid rgb(255 255 255 / 0.06);
    overflow-y: auto;
  }
  @container settings (max-width: 720px) {
    .sidebar {
      flex-basis: 164px;
    }
    kbd {
      display: none;
    }
  }
  button {
    font: inherit;
    color: inherit;
    background: none;
    border: 0;
    cursor: pointer;
  }
  .back {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 28px;
    padding: 0 8px;
    border-radius: 7px;
    color: var(--fg2);
    font-size: 12px;
    font-weight: 500;
  }
  .back:hover {
    color: var(--fg);
    background: rgb(255 255 255 / 0.07);
  }
  .backText {
    flex: 1 1 auto;
    min-width: 0;
    text-align: left;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  kbd {
    font: inherit;
    font-size: 10px;
    font-weight: 600;
    color: var(--fg3);
    padding: 1px 5px;
    border-radius: 4px;
    border: 1px solid rgb(255 255 255 / 0.14);
  }
  .search {
    display: flex;
    align-items: center;
    gap: 4px;
    height: 30px;
    margin-bottom: 4px;
    padding: 0 9px;
    border-radius: 8px;
    background: rgb(255 255 255 / 0.06);
    border: 1px solid rgb(255 255 255 / 0.08);
    transition: border-color 0.15s ease-out;
  }
  .search.focus {
    border-color: rgb(var(--working) / 0.7);
  }
  .search input {
    flex: 1 1 auto;
    min-width: 0;
    font: inherit;
    font-size: 12.5px;
    color: var(--fg);
    background: none;
    border: 0;
    outline: none;
  }
  .search input::placeholder {
    color: var(--fg3);
  }
  .clear {
    display: grid;
    place-items: center;
    color: var(--fg2);
  }
  .sections {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .section {
    display: flex;
    align-items: center;
    gap: 9px;
    width: 100%;
    height: 30px;
    padding: 0 9px;
    border-radius: 7px;
    border: 1px solid transparent;
    color: var(--fg2);
    font-size: 12.5px;
    text-align: left;
  }
  .section:hover {
    background: rgb(255 255 255 / 0.04);
  }
  .section.on {
    color: var(--fg);
    font-weight: 600;
    background: rgb(var(--working) / 0.16);
    border-color: rgb(var(--working) / 0.25);
  }
  .section.dim {
    opacity: 0.4;
  }
  .icon {
    display: grid;
    place-items: center;
    width: 18px;
  }
  .section.on .icon {
    color: rgb(var(--working));
  }
</style>
