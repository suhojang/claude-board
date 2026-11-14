<script lang="ts">
  // 자율주행 실행 한 줄(경로 카드 안): [자율주행] 상태 · 환경 [샌드박스 없음] · 단위 · 시계 · 비용 · 대기 · 통과 수 · 단추들.
  // 글과 단추를 고르는 일은 autopilotView.ts 가 하고, 여기서는 그리고 단추 누름만 백엔드에 넘긴다
  import type { AutopilotRun } from "$lib/types";
  import { autopilotControl } from "$lib/backend";
  import { buttons, lineText } from "./autopilotView";
  import { hint } from "./hint.svelte";

  let { run, now }: { run: AutopilotRun; now: number } = $props();

  const text = $derived(lineText(run, now));
  const actions = $derived(buttons(run));
  // 지금 상태의 이유는 줄에 쓰지 않고 상태 글의 힌트로 둔다
  const stateHint = $derived(run.reason || run.title);

  // 누른 뒤 다음 스냅샷(run 이 새 값으로 바뀔 때)까지 단추를 잠가 같은 요청이 두 번 가지 않게 한다
  let pressed = $state(false);
  $effect(() => {
    void run;
    pressed = false;
  });

  function press(action: "pause" | "resume" | "stop") {
    if (pressed) return;
    pressed = true;
    autopilotControl(run.id, action).catch((e) => {
      console.warn(`자율주행 ${action} 실패`, e);
      pressed = false;
    });
  }
</script>

<div class="line" class:ended={run.ended}>
  <span class="badge">자율주행</span>
  <span class="items">
    <span class="state" use:hint={stateHint}>{text.state}</span>
    <span class="sep">·</span>
    <span>{text.env}</span>
    {#if text.noSandbox}
      <span class="nosandbox" use:hint={"이 실행은 Claude Code 샌드박스 없이 돕니다. 출발할 때 동의했습니다."}>샌드박스 없음</span>
    {/if}
    {#if text.unit}<span class="sep">·</span><span class="unit" use:hint={text.unit}>{text.unit}</span>{/if}
    {#if text.clock}<span class="sep">·</span><span class="num">{text.clock}</span>{/if}
    {#if text.cost}<span class="sep">·</span><span class="num">{text.cost}</span>{/if}
    {#if text.wait}<span class="sep">·</span><span class="num">{text.wait}</span>{/if}
    <span class="sep">·</span><span class="num">{text.tally}</span>
  </span>
  {#if actions.length}
    <span class="buttons">
      {#each actions as b (b.action)}
        <button class="btn" class:stop={b.action === "stop"} disabled={pressed} onclick={() => press(b.action)}>{b.label}</button>
      {/each}
    </span>
  {/if}
</div>

<style>
  .line {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 5px 8px;
    margin-top: 8px;
    padding: 6px 10px;
    border-radius: 10px;
    font-size: 11px;
    line-height: 15px;
    color: var(--fg2);
    background: rgb(255 255 255 / 0.05);
  }
  .line.ended {
    opacity: 0.55;
  }
  .badge {
    flex: none;
    padding: 1.5px 6px;
    border-radius: 999px;
    font-size: 10px;
    font-weight: 600;
    line-height: 13px;
    color: rgb(var(--working));
    background: rgb(var(--working) / 0.15);
    box-shadow: inset 0 0 0 0.5px rgb(var(--working) / 0.25);
  }
  .items {
    flex: 1;
    min-width: 0;
    display: inline-flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 2px 5px;
  }
  .state {
    font-weight: 600;
    color: var(--fg);
  }
  .sep {
    opacity: 0.6;
  }
  .unit {
    min-width: 0;
    max-width: 100%;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .nosandbox {
    --w: 255 179 92;
    flex: none;
    padding: 1.5px 6px;
    border-radius: 999px;
    font-size: 10px;
    font-weight: 600;
    line-height: 13px;
    white-space: nowrap;
    color: rgb(var(--w));
    background: rgb(var(--w) / 0.15);
    box-shadow: inset 0 0 0 0.5px rgb(var(--w) / 0.3);
  }
  .buttons {
    flex: none;
    display: inline-flex;
    gap: 5px;
  }
  .btn {
    padding: 2px 9px;
    border-radius: 999px;
    font-size: 10px;
    font-weight: 600;
    line-height: 13px;
    color: rgb(255 255 255 / 0.85);
    background: rgb(255 255 255 / 0.1);
    transition: background 0.15s;
  }
  .btn:hover:not(:disabled) {
    background: rgb(255 255 255 / 0.2);
  }
  .btn.stop:hover:not(:disabled) {
    color: rgb(255 69 58);
    background: rgb(255 69 58 / 0.18);
  }
  .btn:disabled {
    opacity: 0.45;
  }
</style>
