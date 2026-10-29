<script lang="ts">
  // 설정 화면 — 터미널 영역(제목 띠 아래)을 덮는다(mac SettingsScreen 의 화면). 떠 있지 않으면 아무것도 그리지 않는다.
  // 왼쪽(SettingsSidebar)에서 묶음을 고르거나 검색하고, 오른쪽에 고른 묶음(groups/*)이나 검색 결과가 선다. 줄은 묶음 화면이 SettingsCard 로 그린다.
  // Esc 는 이 화면이 받지 않는다 — 창 전체의 캡처(TerminalAreaView → shortcuts/dispatch.ts)가 settings.escape 로 처리한다.
  // 올라오고 내려가는 움직임은 설정 > 창의 settingsTransition(밀기·서서히·없음)을 따른다 — 규칙은 motion.ts.
  import { tick } from "svelte";
  import { cubicIn, cubicOut } from "svelte/easing";
  import { board } from "$lib/board/store.svelte";
  import { TerminalHub, focusActive } from "$lib/terminal/hub.svelte";
  import { SETTINGS_ITEMS, type SettingsItem } from "./items";
  import { motionFor, frameAt } from "./motion";
  import { SettingsScreen, connectSettings, dismissForCollapse, goSection } from "./screen.svelte";
  import { search } from "./search";
  import { SECTION_ORDER, SETTINGS_SECTIONS, sectionTitle } from "./sections";
  import SettingsSidebar from "./SettingsSidebar.svelte";
  import Account from "./groups/Account.svelte";
  import Board from "./groups/Board.svelte";
  import General from "./groups/General.svelte";
  import Notify from "./groups/Notify.svelte";
  import Shortcuts from "./groups/Shortcuts.svelte";
  import Terminal from "./groups/Terminal.svelte";
  import Update from "./groups/Update.svelte";
  import Window from "./groups/Window.svelte";

  let contentEl: HTMLElement | undefined = $state();

  $effect(() => connectSettings());

  // 터미널 칸이 접히면(⌃⌥T·접기 단추) 설정 화면도 효과 없이 내려간다
  $effect(() => {
    void SettingsScreen.shown;
    void board.snap?.settings.terminalExpanded;
    dismissForCollapse();
  });

  const current = $derived(SETTINGS_SECTIONS.find((section) => section.id === SettingsScreen.section) ?? SETTINGS_SECTIONS[0]);
  const query = $derived(SettingsScreen.query.trim());
  const found = $derived(search(SETTINGS_ITEMS, query));
  const foundBySection = $derived(
    SECTION_ORDER.map((id) => ({ id, items: found.filter((item) => item.section === id) })).filter((entry) => entry.items.length > 0),
  );

  const prefersReducedMotion = () => typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches;

  // 전환 — 켜는 순간 설정값을 읽는다(열린 채 바꾼 값이 닫을 때 바로 적용된다)
  function slide(_node: HTMLElement, params: { out: boolean }) {
    const motion = motionFor(board.snap?.settings.settingsTransition, {
      boardLeft: board.snap?.settings.boardSide === "left",
      reduced: prefersReducedMotion(),
    });
    return {
      duration: motion.duration,
      easing: params.out ? cubicIn : cubicOut, // 빠르게 들어와 부드럽게 멈추고, 나갈 때는 천천히 시작한다
      css: (t: number) => {
        const frame = frameAt(motion, t);
        return `transform: translateX(${frame.translatePct}%); opacity: ${frame.opacity};`;
      },
    };
  }

  function onOutroEnd() {
    SettingsScreen.visible = false;
    // 탭을 다루는 동작이 아니라 돌아가기·Esc 로 닫았으면 터미널로 포커스를 돌려준다. 새 세션 창·확인 창이 떠 있으면 그쪽이 포커스를 갖는다
    if (SettingsScreen.refocus && !SettingsScreen.shown && !TerminalHub.sheet && !TerminalHub.confirm) focusActive();
  }

  // 검색 결과를 눌렀다 — 그 묶음으로 가서 그 줄을 보여 주고 잠깐 밝힌다(줄은 카드 제목으로 찾는다).
  // 맞는 카드가 없으면(예: 카드 제목이 계정 이메일인 "저장한 계정") 묶음을 고른 것으로 끝나며 화면은 묶음 맨 위로 둔다
  async function jumpTo(item: SettingsItem) {
    goSection(item.section);
    await tick();
    const card = Array.from(contentEl?.querySelectorAll<HTMLElement>('[data-testid="settings-card"]') ?? []).find((el) => el.dataset.title === item.title);
    if (!card) {
      contentEl?.scrollTo({ top: 0 });
      return;
    }
    const reduced = prefersReducedMotion();
    card.scrollIntoView({ block: "center", behavior: reduced ? "auto" : "smooth" });
    if (!reduced) card.animate([{ outline: "2px solid rgb(115 168 255 / 0.9)" }, { outline: "2px solid rgb(115 168 255 / 0)" }], { duration: 1200, easing: "ease-out" });
  }
</script>

<div class="clip">
  {#if SettingsScreen.shown}
    <section
      class="settings"
      data-testid="settings-view"
      aria-label="설정"
      in:slide={{ out: false }}
      out:slide={{ out: true }}
      onoutroend={onOutroEnd}
    >
      <SettingsSidebar />
      <main class="content" bind:this={contentEl} aria-labelledby="settings-heading">
        <div class="column">
          {#if query === ""}
            <header>
              <h1 id="settings-heading">{current.title}</h1>
              <p>{current.subtitle}</p>
            </header>
            <div class="group" data-section={SettingsScreen.section}>
              {#if SettingsScreen.section === "general"}
                <General />
              {:else if SettingsScreen.section === "window"}
                <Window />
              {:else if SettingsScreen.section === "notify"}
                <Notify />
              {:else if SettingsScreen.section === "terminal"}
                <Terminal />
              {:else if SettingsScreen.section === "board"}
                <Board />
              {:else if SettingsScreen.section === "account"}
                <Account />
              {:else if SettingsScreen.section === "shortcuts"}
                <Shortcuts />
              {:else}
                <Update />
              {/if}
            </div>
          {:else}
            <h1 id="settings-heading" class="resultsTitle" aria-live="polite" data-testid="settings-result-count">"{query}" 검색 결과 {found.length}개</h1>
            {#if found.length === 0}
              <div class="empty" role="status">
                <strong>맞는 설정이 없습니다</strong>
                <span>다른 낱말로 찾아 보세요</span>
              </div>
            {/if}
            {#each foundBySection as entry (entry.id)}
              <section class="results" data-testid={`settings-results-${entry.id}`} aria-label={sectionTitle(entry.id)}>
                <button class="resultsHead" type="button" onclick={() => goSection(entry.id)}>{sectionTitle(entry.id)} ›</button>
                <ul>
                  {#each entry.items as item (item.id)}
                    <li><button class="result" type="button" data-testid="settings-result" onclick={() => jumpTo(item)}>{item.title}</button></li>
                  {/each}
                </ul>
              </section>
            {/each}
          {/if}
        </div>
      </main>
    </section>
  {/if}
</div>

<style>
  /* 터미널 칸 위를 덮는다 — 제목 띠(32px) 아래부터. 밀려 들어오는 동안 칸 밖(보드 쪽)으로 번지지 않게 잘라 둔다 */
  .clip {
    position: absolute;
    inset: 32px 0 0;
    z-index: 20;
    overflow: hidden;
    pointer-events: none;
  }
  .settings {
    position: absolute;
    inset: 0;
    display: flex;
    pointer-events: auto;
    background: var(--term-bar);
    color: var(--fg);
    container: settings / inline-size;
  }
  .content {
    flex: 1 1 0;
    min-width: 0;
    overflow-y: auto;
    background: #121318;
  }
  .column {
    max-width: 660px;
    margin: 0 auto;
    padding: 26px 32px 40px;
  }
  @container settings (max-width: 720px) {
    .column {
      padding-inline: 18px;
    }
  }
  header {
    padding-bottom: 20px;
    margin-bottom: 20px;
    border-bottom: 1px solid rgb(255 255 255 / 0.06);
  }
  h1 {
    margin: 0;
    font-size: 22px;
    font-weight: 700;
  }
  header p {
    margin: 5px 0 0;
    font-size: 12px;
    color: var(--fg2);
  }
  .group {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .resultsTitle {
    margin-bottom: 22px;
    font-size: 12px;
    font-weight: 400;
    color: var(--fg2);
  }
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    padding-top: 60px;
    font-size: 11.5px;
    color: var(--fg2);
  }
  .empty strong {
    font-size: 13px;
    color: var(--fg);
  }
  .results {
    margin-bottom: 26px;
  }
  .results ul {
    margin: 10px 0 0;
    padding: 0;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  button {
    font: inherit;
    color: inherit;
    background: none;
    border: 0;
    cursor: pointer;
  }
  .resultsHead {
    padding: 0;
    font-size: 15px;
    font-weight: 700;
  }
  .result {
    width: 100%;
    padding: 10px 14px;
    text-align: left;
    font-size: 13px;
    font-weight: 600;
    border-radius: 10px;
    background: rgb(255 255 255 / 0.035);
    border: 1px solid rgb(255 255 255 / 0.07);
  }
  .result:hover {
    background: rgb(255 255 255 / 0.07);
  }
</style>
