<script lang="ts">
  // 세션 한 줄(ClaudeBoard.swift:1743-1913): 서브에이전트 접기 화살표·상태 아이콘·제목·알림 스위치·… 메뉴(종료면 ⓧ),
  // 둘째 줄에 경과 시간·하위 작업·셸·세션 이름·모델·컨텍스트(좁으면 3단계로 줄이고 세션 이름은 앞 두 단계에만 보임), 승인 대기면 ✋ 요청 내용
  import { cubicOut } from "svelte/easing";
  import { fly, fade } from "svelte/transition";
  import type { Session } from "$lib/types";
  import { removeSession, sessionAction, toggleSubCollapsed } from "$lib/backend";
  import ContextLabel from "./ContextLabel.svelte";
  import Fit from "./Fit.svelte";
  import Icon from "./Icon.svelte";
  import ModelLabel from "./ModelLabel.svelte";
  import NameLabel from "./NameLabel.svelte";
  import MuteSwitch from "./MuteSwitch.svelte";
  import PlaceChip from "./PlaceChip.svelte";
  import StateIcon from "./StateIcon.svelte";
  import SubagentRow from "./SubagentRow.svelte";
  import { clock, elapsed, stateName, stateRgb } from "./format";
  import { hint } from "./hint.svelte";
  import { openMenu, type MenuItem } from "./menu.svelte";

  let { s, now, locked, subCollapsed }: { s: Session; now: number; locked: boolean; subCollapsed: boolean } = $props();

  const subOpen = $derived(!subCollapsed);
  const ended = $derived(s.state === "ended");
  const running = $derived(s.subagents.filter((x) => x.state !== "done").length);

  function toggleSubs(e: MouseEvent) {
    e.stopPropagation();
    toggleSubCollapsed(s.id);
  }

  // 한 번 클릭 = 그 세션의 터미널 앱만 앞으로, 두 번 클릭 = 종료 세션은 다시 열기, 아니면 그 세션 창으로 이동.
  // 클릭 횟수로 나눈다 — 한 번·두 번 클릭을 따로 기다리면 줄 안의 스위치·단추까지 늦게 반응한다
  function click(e: MouseEvent) {
    if (e.detail >= 2) sessionAction(ended ? "resume" : "focus", s.id);
    else sessionAction("activate", s.id);
  }

  function showMenu(e: MouseEvent) {
    e.stopPropagation();
    let items: MenuItem[];
    if (s.state === "ended") {
      items = [{ label: s.paused ? "세션 재개 (이어서 진행)" : "세션 다시 열기", action: () => sessionAction("resume", s.id) }];
    } else if (s.state === "paused") {
      items = [
        { label: "세션 재개", action: () => sessionAction("resume_paused", s.id) },
        { separator: true },
        { label: "세션 중단", action: () => sessionAction("stop", s.id) },
      ];
    } else {
      items = [
        { label: "일시중지", enabled: s.state !== "idle" && s.canDrive, action: () => sessionAction("pause", s.id) },
        { separator: true },
        { label: "세션 중단", action: () => sessionAction("stop", s.id) },
      ];
    }
    openMenu(items, e);
  }

  const stop = (e: Event) => e.stopPropagation();
</script>

<div class="session">
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div class="row" role="button" tabindex="-1" style:--c={stateRgb(s.state)} onclick={click}>
    <div class="lead">
      {#if s.subagents.length === 0}
        <span class="slot"></span>
      {:else}
        <button
          class="chev"
          class:open={subOpen}
          onclick={toggleSubs}
          ondblclick={stop}
          use:hint={subOpen ? "서브에이전트 목록 접기" : "서브에이전트 목록 펼치기"}
        >
          <Icon name="chevron-right" size={11} />
        </button>
      {/if}
      <span class="icon" use:hint={stateName(s.state)}><StateIcon state={s.state} /></span>
    </div>
    <div class="main">
      <div class="top">
        <span class="title" class:dim={ended}>{s.title}</span>
        {#if !ended}<MuteSwitch {s} {locked} />{/if}
        <button class="more" onclick={showMenu} ondblclick={stop} use:hint={"세션 일시중지·재개·중단"}>
          <Icon name="ellipsis" size={12} />
        </button>
        {#if ended}
          <button
            class="remove"
            onclick={(e) => {
              e.stopPropagation();
              removeSession(s.id);
            }}
            ondblclick={stop}
            use:hint={"목록에서 지우기"}
          >
            <Icon name="xmark-circle" size={14} />
          </button>
        {/if}
      </div>
      <!-- 다 들어가면 글자까지, 좁으면 하위 작업은 아이콘 + 개수만, 더 좁으면 컨텍스트 막대·토큰을 뺀다 -->
      <Fit count={3} class="meta">
        {#snippet content(level)}
          <span class="items">
            <span class="num el">{ended ? `${clock(s.updated)} 종료` : s.state === "idle" ? `${elapsed(s.since, now)} 전` : elapsed(s.since, now)}</span>
            {#if s.place}<PlaceChip place={s.place} headless={s.headless} />{/if}
            {#if s.subagents.length || s.shells > 0 || s.ctx || s.brain}<span>·</span>{/if}
            {#if s.subagents.length}
              {@const n = running > 0 && running < s.subagents.length ? `${running}/${s.subagents.length}` : `${s.subagents.length}`}
              <button
                class="subtoggle num"
                onclick={toggleSubs}
                ondblclick={stop}
                use:hint={`하위 작업 ${s.subagents.length}개 — ` + (subOpen ? "서브에이전트 목록 접기" : "서브에이전트 목록 펼치기")}
              >
                <Icon name="people" size={12.6} pad={1.45} />
                {level > 0 ? n : `하위 작업 ${n}`}
                {#if level === 0}<span class="fold">{subOpen ? "· 접기" : "· 펼치기"}</span>{/if}
              </button>
            {/if}
            {#if s.shells > 0}
              <span class="shells" use:hint={`셸 ${s.shells}개 실행 중`}>
                <Icon name="shells" size={11} />{level > 0 ? `${s.shells}` : `셸 ${s.shells}개 실행 중`}
              </span>
            {/if}
            {#if s.name && !ended && level < 2}<NameLabel name={s.name} />{/if}
            {#if s.brain}<ModelLabel brain={s.brain} {level} />{/if}
            {#if s.ctx}<ContextLabel ctx={s.ctx} {level} />{/if}
          </span>
        {/snippet}
      </Fit>
      {#if s.state === "waiting" && s.message}
        <div class="msg" transition:fly={{ y: -6, duration: 200 }}>
          <Icon name="hand" size={11} />
          <span>{s.message}</span>
        </div>
      {/if}
    </div>
    {#if s.state === "working"}<span class="shimmer"></span>{/if}
  </div>
  {#if subOpen}
    {#each s.subagents as sub, i (sub.id)}
      <div
        in:fly|global={{ y: -10, duration: 380, delay: Math.min(i, 8) * 35, easing: cubicOut }}
        out:fade={{ duration: 150 }}
      >
        <SubagentRow {sub} {now} last={i === s.subagents.length - 1} />
      </div>
    {/each}
  {/if}
</div>

<style>
  .row {
    position: relative;
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 8px 10px;
    border-radius: 10px;
    transition: background 0.15s ease-out;
    outline: none;
  }
  .row:hover {
    background: rgb(255 255 255 / 0.07);
  }
  .lead {
    display: flex;
    align-items: center;
    gap: 4px;
    padding-top: 1px;
    flex: none;
  }
  .slot {
    width: 14px;
    height: 18px;
  }
  .chev {
    width: 14px;
    height: 18px;
    display: grid;
    place-items: center;
    border-radius: 4px;
    color: rgb(255 255 255 / 0.65);
    transition: background 0.15s;
  }
  .chev :global(svg) {
    transition: transform 0.3s cubic-bezier(0.3, 1.2, 0.5, 1);
  }
  .chev.open :global(svg) {
    transform: rotate(90deg);
  }
  .chev:hover {
    color: #fff;
    background: rgb(255 255 255 / 0.12);
  }
  .icon {
    display: inline-flex;
  }
  .main {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .top {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 20px;
  }
  /* SwiftUI 는 제목과 스위치 사이에 Spacer(최소 4)와 간격 둘이 끼어 제목이 스위치에서 12px 더 떨어져 잘린다 */
  .title {
    flex: 1;
    min-width: 0;
    margin-right: 12px;
    font-size: 13px;
    font-weight: 500;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .title.dim {
    color: var(--fg2);
  }
  .more {
    flex: none;
    width: 26px;
    height: 22px;
    border-radius: 11px;
    display: grid;
    place-items: center;
    color: rgb(255 255 255 / 0.55);
    background: rgb(255 255 255 / 0.06);
    transition:
      transform 0.25s cubic-bezier(0.3, 1.6, 0.5, 1),
      background 0.15s,
      color 0.15s;
  }
  .more:hover {
    transform: scale(1.3);
    color: rgb(255 255 255 / 0.95);
    background: rgb(255 255 255 / 0.16);
  }
  .remove {
    flex: none;
    display: grid;
    place-items: center;
    color: var(--fg2);
    opacity: 0.5;
  }
  .row:hover .remove {
    opacity: 1;
  }
  .main :global(.meta) {
    font-size: 11px;
    color: var(--fg2);
  }
  .items {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    white-space: nowrap;
  }
  .el {
    margin-right: 2px;
  }
  .subtoggle {
    --s: 242 140 191;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 1.5px 6px;
    font-size: 10px;
    font-weight: 600;
    line-height: 13px;
    color: rgb(var(--s));
    background: rgb(var(--s) / 0.15);
    box-shadow: inset 0 0 0 0.5px rgb(var(--s) / 0.25);
    border-radius: 999px;
    transition: background 0.15s;
  }
  .subtoggle:hover {
    background: rgb(var(--s) / 0.28);
  }
  .subtoggle .fold {
    color: rgb(255 255 255 / 0.6);
  }
  .subtoggle:hover .fold {
    color: rgb(255 255 255 / 0.9);
  }
  .shells {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    color: rgb(var(--working));
  }
  .msg {
    display: flex;
    align-items: flex-start;
    gap: 6px;
    font-size: 11px;
    font-weight: 500;
    color: rgb(var(--c));
  }
  .msg :global(svg) {
    margin-top: 1px;
  }
  .msg span {
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    overflow-wrap: anywhere;
  }
</style>
