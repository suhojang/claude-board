<script lang="ts">
  // 서브에이전트 한 줄(ClaudeBoard.swift SubagentRow) — 트리 연결선, 작은 상태 아이콘, 설명(없으면 종류),
  // 아래 줄에 라벨 "서브에이전트"·종류·경과·모델·🔧도구. 좁으면 라벨을 아이콘만, 더 좁으면 종류·도구 이름을 뺀다
  import type { Sub } from "$lib/types";
  import Fit from "./Fit.svelte";
  import Icon from "./Icon.svelte";
  import KindLabel from "./KindLabel.svelte";
  import ModelLabel from "./ModelLabel.svelte";
  import StateIcon from "./StateIcon.svelte";
  import { elapsed, subState } from "./format";
  import { hint } from "./hint.svelte";

  let { sub, now, last }: { sub: Sub; now: number; last: boolean } = $props();
  const done = $derived(sub.state === "done");
</script>

<div
  class="sub"
  class:done
  use:hint={sub.background ? "백그라운드로 실행된 서브에이전트" : "세션이 결과를 기다리는 서브에이전트"}
>
  <span class="branch" class:last></span>
  <!-- SwiftUI scaleEffect 는 칸 크기를 그대로 두고 그림만 줄인다 — 18 칸 안에서 0.8 배 -->
  <span class="ico"><StateIcon state={subState(sub)} scale={0.8} /></span>
  <div class="col">
    <div class="desc">{sub.desc || sub.type}</div>
    <Fit count={3} class="meta">
      {#snippet content(level)}
        <span class="items">
          <KindLabel subagent compact={level > 0} />
          {#if level < 2 && sub.type}<span>{sub.type}</span>{/if}
          {#if level < 2}<span>·</span>{/if}
          <span class="num">
            {done ? `${elapsed(sub.started, sub.ended ?? now)} 걸려 완료` : sub.state === "waiting" ? "승인 대기" : elapsed(sub.started, now)}
          </span>
          {#if sub.brain}<ModelLabel brain={sub.brain} {level} />{/if}
          {#if !done && sub.tool}
            <span>·</span>
            <span class="tool" use:hint={sub.tool}><Icon name="wrench" size={15} pad={-1.25} />{#if level < 2}{sub.tool}{/if}</span>
          {/if}
        </span>
      {/snippet}
    </Fit>
  </div>
</div>

<style>
  .sub {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 38px;
    padding: 0 10px 0 36px;
  }
  .sub.done {
    opacity: 0.6;
  }
  /* 트리 연결선: 위에서 내려와 오른쪽으로 꺾이고, 마지막 줄이 아니면 아래로 계속 이어진다 */
  .branch {
    position: relative;
    flex: none;
    width: 16px;
    align-self: stretch;
  }
  .branch::before {
    content: "";
    position: absolute;
    left: 7.4px;
    top: 0;
    bottom: 0;
    border-left: 1.2px solid rgb(255 255 255 / 0.22);
  }
  .branch::after {
    content: "";
    position: absolute;
    left: 7.4px;
    right: 0;
    top: calc(50% - 0.6px);
    border-top: 1.2px solid rgb(255 255 255 / 0.22);
  }
  .branch.last::before {
    bottom: calc(50% - 0.6px);
    right: 0;
    border-bottom: 1.2px solid rgb(255 255 255 / 0.22);
    border-bottom-left-radius: 6px;
  }
  .branch.last::after {
    display: none;
  }
  .ico {
    flex: none;
    display: grid;
    place-items: center;
    width: 18px;
    height: 18px;
  }
  .col {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .desc {
    font-size: 12px;
    font-weight: 500;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .done .desc {
    color: var(--fg2);
  }
  .col :global(.meta) {
    font-size: 10.5px;
    color: var(--fg2);
  }
  .items {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    white-space: nowrap;
  }
  .tool {
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }
</style>
