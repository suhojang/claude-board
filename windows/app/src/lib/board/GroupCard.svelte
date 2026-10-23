<script lang="ts">
  // 경로 카드(ClaudeBoard.swift GroupCard): 머리(접기 화살표·폴더 또는 대표 상태·이름·경로·별·X·상태별 개수, 오른쪽 클릭 메뉴),
  // 안에 터미널/백그라운드 구역과 세션 줄, "종료된 세션 n개"(접기, 마우스를 올리면 휴지통)
  import { cubicOut } from "svelte/easing";
  import { fade, fly, slide } from "svelte/transition";
  import type { BoardSettings, Group } from "$lib/types";
  import { place } from "$lib/terminal/hub.svelte";
  import { DEFAULT_SESSION_COMMAND, startSession } from "$lib/terminal/launch";
  import { hideGroup, removeEnded, toggleEndedOpen, toggleKindCollapsed, togglePinGroup } from "$lib/backend";
  import HeaderButton from "./HeaderButton.svelte";
  import Icon from "./Icon.svelte";
  import KindSection from "./KindSection.svelte";
  import SessionRow from "./SessionRow.svelte";
  import StateIcon from "./StateIcon.svelte";
  import { active, doing, stateName, stateRgb, urgent } from "./format";
  import { hint } from "./hint.svelte";
  import { openMenu } from "./menu.svelte";

  let {
    g,
    now,
    collapsed,
    toggle,
    settings,
    locked,
  }: { g: Group; now: number; collapsed: boolean; toggle: () => void; settings: BoardSettings; locked: boolean } = $props();

  const top = $derived(urgent(g));
  const c = $derived(stateRgb(top));
  const busy = $derived(active(g));
  const lead = $derived(busy[0]);
  const pinned = $derived(settings.pinnedGroups.includes(g.id));
  const endedOpen = $derived(settings.endedOpen.includes(g.id));
  const live = $derived(g.sessions.filter((s) => s.state !== "ended"));
  // 구역은 세션이 도는 자리로 나눈다(mac 구역 머리글) — 보드 탭 안에서 시작한 claude -p 는 터미널 구역이다
  const terminal = $derived(live.filter((s) => place(s).kind !== "background"));
  const background = $derived(live.filter((s) => place(s).kind === "background"));
  const ended = $derived(g.sessions.filter((s) => s.state === "ended"));
  const termFolded = $derived(settings.kindCollapsed.includes(`${g.id}|term`));
  const bgFolded = $derived(settings.kindCollapsed.includes(`${g.id}|bg`));
  const counts = $derived(
    (["waiting", "working", "idle"] as const)
      .map((st) => ({ st, n: g.sessions.filter((s) => s.state === st).length }))
      .filter((x) => x.n > 0),
  );

  let endedHover = $state(false);
  let trashHover = $state(false);

  function contextMenu(e: MouseEvent) {
    e.preventDefault();
    openMenu(
      [
        { label: pinned ? "상단 고정 풀기" : "상단에 고정", action: () => togglePinGroup(g.id) },
        { label: "목록에서 지우기", destructive: true, action: () => hideGroup(g.id) },
      ],
      e,
    );
  }

  // 펼칠 때 줄마다 조금씩 늦게 위에서 내려오며 나타난다
  const rowIn = (i: number) => ({ y: -10, duration: 380, delay: Math.min(i, 8) * 35, easing: cubicOut });
</script>

<div class="card" class:waiting={top === "waiting"} class:folded={collapsed} style:--c={c}>
  <div
    class="head"
    role="button"
    tabindex="-1"
    onclick={toggle}
    onkeydown={(e) => e.key === "Enter" && toggle()}
    oncontextmenu={contextMenu}
  >
    <span class="chev" class:open={!collapsed} use:hint={collapsed ? "경로 카드 펼치기" : "경로 카드 접기"}>
      <Icon name="chevron-right" size={12.5} />
    </span>
    <!-- 접혀 있어도 작업 중(도는 링)·승인 대기(퍼지는 파문)가 보이게 폴더 자리에 상태 표시를 그린다 -->
    <span
      class="folder"
      use:hint={collapsed ? (lead ? `${stateName(lead.state)} · 누르면 펼치기` : "작업 경로 · 누르면 펼치기") : "작업 경로 · 누르면 접기"}
    >
      {#if collapsed && lead}
        <StateIcon state={lead.state} />
      {:else}
        <Icon name="folder" size={17.5} />
      {/if}
    </span>
    <div class="names">
      <div class="name">{g.name}</div>
      {#if collapsed && lead}
        <!-- 접혀 있으면 경로 대신 지금 하는 일을 상태 색으로 — 여럿이면 나머지 개수를 붙인다 -->
        <div class="doing" style:color="rgb({stateRgb(lead.state)})">
          <span class="text">{stateName(lead.state)} · {doing(lead)}</span>
          {#if busy.length > 1}<span class="more">+{busy.length - 1}</span>{/if}
        </div>
      {:else}
        <!-- 긴 경로는 앞을 줄인다(…\work\shop-api) — rtl 칸 안에 ltr 로 묶어 기호 순서가 뒤집히지 않게 -->
        <div class="path"><bdi>{g.path}</bdi></div>
      {/if}
    </div>
    <HeaderButton
      icon="plus"
      help={`이 경로에서 새 세션 (${DEFAULT_SESSION_COMMAND})`}
      onclick={() => startSession(DEFAULT_SESSION_COMMAND, g.id)}
    />
    <HeaderButton
      icon={pinned ? "star-fill" : "star"}
      tint={pinned ? "255 199 71" : null}
      help={pinned ? "상단 고정 풀기" : "상단에 고정"}
      onclick={() => togglePinGroup(g.id)}
    />
    <HeaderButton icon="xmark" help="목록에서 지우기 — 새 활동이 생기면 다시 나타납니다" onclick={() => hideGroup(g.id)} />
    <span class="counts">
      {#each counts as x (x.st)}
        <span class="count num" use:hint={`${stateName(x.st)} ${x.n}개`}>
          <span class="dot" style:background="rgb({stateRgb(x.st)})"></span>{x.n}
        </span>
      {/each}
    </span>
    <!-- 접힌 채 작업 중이면 세션 줄처럼 빛이 지나간다 -->
    {#if collapsed && top === "working"}<span class="shimmer"></span>{/if}
  </div>

  {#if !collapsed}
    <div class="body" transition:slide={{ duration: 380, easing: cubicOut }}>
      <!-- 터미널 세션과 백그라운드 세션을 구역으로 나눈다 — 백그라운드는 더블 클릭으로 이동할 터미널이 없다 -->
      {#if terminal.length}
        <KindSection headless={false} sessions={terminal} collapsed={termFolded} toggle={() => toggleKindCollapsed(`${g.id}|term`)} />
      {/if}
      {#if !termFolded}
        {#each terminal as s, i (s.id)}
          <div in:fly|global={rowIn(i)} out:fade={{ duration: 150 }}>
            <SessionRow {s} {now} {locked} subCollapsed={settings.subCollapsed.includes(s.id)} />
          </div>
        {/each}
      {/if}
      {#if background.length}
        <KindSection headless={true} sessions={background} collapsed={bgFolded} toggle={() => toggleKindCollapsed(`${g.id}|bg`)} />
      {/if}
      {#if !bgFolded}
        {#each background as s, i (s.id)}
          <div in:fly|global={rowIn(terminal.length + i)} out:fade={{ duration: 150 }}>
            <SessionRow {s} {now} {locked} subCollapsed={settings.subCollapsed.includes(s.id)} />
          </div>
        {/each}
      {/if}
      {#if ended.length}
        <div
          class="ended"
          class:hover={endedHover}
          role="button"
          tabindex="-1"
          onmouseenter={() => (endedHover = true)}
          onmouseleave={() => (endedHover = false)}
          onclick={() => toggleEndedOpen(g.id)}
          onkeydown={(e) => e.key === "Enter" && toggleEndedOpen(g.id)}
          use:hint={endedOpen ? "종료된 세션 접기" : "종료된 세션 펼치기"}
        >
          <span class="chev small" class:open={endedOpen}><Icon name="chevron-right" size={8} /></span>
          <span class="label num">종료된 세션 {ended.length}개</span>
          {#if endedHover}
            <!-- 그 작업경로의 종료된 세션 기록을 한 번에 지운다 — 대화 기록(jsonl)은 남는다 -->
            <button
              class="trash"
              class:hover={trashHover}
              onmouseenter={() => (trashHover = true)}
              onmouseleave={() => (trashHover = false)}
              onclick={(e) => {
                e.stopPropagation();
                removeEnded(g.id);
              }}
              transition:fly={{ x: 8, duration: 150 }}
              use:hint={`종료된 세션 ${ended.length}개를 목록에서 지우기`}
            >
              <Icon name="trash" size={10} />
              {#if trashHover}<span>모두 지우기</span>{/if}
            </button>
          {/if}
        </div>
        {#if endedOpen}
          <div class="endedList" transition:slide={{ duration: 300, easing: cubicOut }}>
            {#each ended as s, i (s.id)}
              <div in:fly|global={rowIn(i)} out:fade={{ duration: 150 }}>
                <SessionRow {s} {now} {locked} subCollapsed={settings.subCollapsed.includes(s.id)} />
              </div>
            {/each}
          </div>
        {/if}
      {/if}
    </div>
  {/if}
</div>

<style>
  .card {
    position: relative;
    padding: 12px;
    border-radius: 16px;
    background: linear-gradient(135deg, rgb(var(--c) / 0.1), rgb(255 255 255 / 0.03));
    box-shadow:
      inset 0 0 0 1px rgb(var(--c) / 0.22),
      0 3px 8px rgb(0 0 0 / 0.03);
    transition:
      box-shadow 0.4s ease-in-out,
      background 0.4s ease-in-out;
  }
  /* 접힌 카드는 macOS 쪽이 1px 더 높다(펼친 카드는 머리 아래 2px 여백이 따로 있어 맞는다) */
  .card.folded {
    padding-bottom: 13px;
  }
  /* 승인 대기가 있으면 테두리와 빛이 2.4초 주기로 천천히 숨 쉰다 */
  .card.waiting {
    box-shadow: inset 0 0 0 1px rgb(var(--c) / 0.25);
  }
  .card.waiting::after {
    content: "";
    position: absolute;
    inset: 0;
    border-radius: 16px;
    pointer-events: none;
    box-shadow:
      inset 0 0 0 1px rgb(var(--c) / 0.55),
      0 3px 12px rgb(var(--c) / 0.35);
    animation: breathe 2.4s ease-in-out infinite;
  }
  @keyframes breathe {
    0%,
    100% {
      opacity: 0;
    }
    50% {
      opacity: 1;
    }
  }
  .head {
    position: relative;
    display: flex;
    align-items: center;
    gap: 10px;
    margin: -6px;
    padding: 6px;
    border-radius: 10px;
    cursor: pointer;
    outline: none;
    transition: background 0.15s;
  }
  .head:hover {
    background: rgb(255 255 255 / 0.06);
  }
  .chev {
    display: grid;
    place-items: center;
    width: 10px;
    margin: 0 1.5px 0 -1.5px;
    flex: none;
    color: var(--fg2);
    opacity: 0.6;
  }
  .head:hover > .chev {
    opacity: 1;
  }
  .chev :global(svg) {
    transition: transform 0.38s cubic-bezier(0.3, 1.2, 0.5, 1);
  }
  .chev.open :global(svg) {
    transform: rotate(90deg);
  }
  .chev.small {
    width: 8px;
    opacity: 1;
    color: inherit;
  }
  .folder {
    flex: none;
    width: 30px;
    height: 30px;
    display: grid;
    place-items: center;
    border-radius: 8px;
    color: rgb(var(--c));
    background: rgb(var(--c) / 0.16);
  }
  /* SwiftUI 는 이름 칸과 단추 사이에 Spacer(최소 8)와 간격 둘이 끼어 이름이 단추에서 18px 더 떨어져 잘린다 */
  .names {
    flex: 1;
    min-width: 0;
    margin-right: 18px;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .name {
    font-size: 15px;
    line-height: 19px;
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .path {
    font-size: 11px;
    line-height: 14px;
    color: var(--fg2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    direction: rtl;
    text-align: left;
    padding-right: 3.75px;
  }
  .doing {
    line-height: 15px;
    display: flex;
    gap: 4px;
    font-size: 11px;
    font-weight: 500;
    min-width: 0;
  }
  .doing .text {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .doing .more {
    flex: none;
  }
  .counts {
    display: flex;
    gap: 6px;
    flex: none;
    padding-right: 1.5px;
    color: var(--fg2);
  }
  .count {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    font-size: 11px;
    font-weight: 600;
  }
  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
  }
  .body {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding-top: 8px;
  }
  .ended {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 10px 6px;
    border-radius: 8px;
    min-height: 25px;
    color: var(--fg2);
    opacity: 0.7;
    cursor: pointer;
    outline: none;
  }
  .ended.hover {
    opacity: 1;
    background: rgb(255 255 255 / 0.06);
  }
  .ended .label {
    flex: 1;
    line-height: 15px;
    font-size: 11px;
    font-weight: 500;
  }
  .trash {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    padding: 3px 7px;
    border-radius: 999px;
    font-size: 10px;
    font-weight: 600;
    color: rgb(255 255 255 / 0.8);
    background: rgb(255 255 255 / 0.1);
  }
  .trash.hover {
    color: rgb(255 69 58);
    background: rgb(255 69 58 / 0.18);
  }
  .endedList {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
</style>
