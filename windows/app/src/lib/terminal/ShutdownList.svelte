<script lang="ts">
  // 끌 때 확인 창(mac ShutdownList) — 꺼지는 보드 탭 세션 목록을 보이고 끌지 묻는다. 보드 창 안의 모달이다(D3).
  // 목록 규칙은 shutdownList.ts, 답은 lifecycle 의 answerConfirm 이 백엔드로 넘긴다.
  import type { ShutdownItem } from "$lib/types";
  import { confirmKeyAction } from "./keyGuards";
  import { Lifecycle, answerConfirm } from "./lifecycle.svelte";

  const KIND_LABEL: Record<ShutdownItem["kind"], string> = {
    working: "작업 중",
    waiting: "승인 대기",
    subagents: "서브에이전트 실행 중",
    shell: "셸에서 실행 중",
    headlessInTab: "claude -p",
  };
  // 색은 보드의 상태 색을 그대로 쓴다 — 작업 중 파랑, 승인 대기 주황, 그 밖은 쉬는 색
  const KIND_COLOR: Record<ShutdownItem["kind"], string> = {
    working: "var(--working)",
    waiting: "var(--waiting)",
    subagents: "var(--working)",
    shell: "var(--idle)",
    headlessInTab: "var(--working)",
  };

  const confirm = $derived(Lifecycle.confirm);
  const update = $derived(confirm?.reason === "update");
  let cancelButton = $state<HTMLButtonElement>();

  // 창이 뜨면 취소 단추에 포커스를 둔다 — 터미널에 포커스가 남아 있으면 Enter 가 뒤에서 도는 프로그램으로 간다
  $effect(() => {
    if (confirm) cancelButton?.focus();
  });

  // xterm 이 Esc 를 먼저 먹어 버블 단계에는 안 오므로 캡처로 받는다. 창 밖 키는 삼킨다(keyGuards.ts)
  function onKey(e: KeyboardEvent) {
    const insideDialog = e.target instanceof Element && e.target.closest("[role=alertdialog]") !== null;
    const action = confirmKeyAction(e.key, insideDialog);
    if (action === "pass") return;
    e.preventDefault();
    e.stopPropagation();
    if (action === "cancel") answerConfirm(false);
  }
</script>

<svelte:window onkeydowncapture={confirm ? onKey : undefined} />

{#if confirm}
  {@const title = `작업 중인 세션 ${confirm.items.length}개가 꺼집니다`}
  <div class="scrim" role="presentation">
    <div class="sheet" role="alertdialog" aria-label={title}>
      <div class="title">{title}</div>
      <div class="text">
        보드가 터미널을 쥐고 있어서 {update ? "업데이트하면" : "보드를 끄면"} 진행 중인 응답과 탭에서 도는 셸·서브에이전트가 끊깁니다.
        보드가 다시 뜨면 대화를 이어 엽니다. 끊긴 세션은 일시중지로 남아 "세션 재개"로 이어 갈 수 있습니다.
      </div>
      <ul class="list" data-testid="shutdown-items">
        {#each confirm.items as item, index (index)}
          <li>
            <span class="dot" style:background={KIND_COLOR[item.kind]}></span>
            <span class="name">{item.title}</span>
            <span class="kind">{KIND_LABEL[item.kind]}</span>
          </li>
        {/each}
      </ul>
      <div class="note">백그라운드 세션과 다른 터미널의 세션은 영향 없이 그대로 돕니다.</div>
      <div class="buttons">
        <button class="primary" onclick={() => answerConfirm(true)}>{update ? "업데이트" : "종료"}</button>
        <button bind:this={cancelButton} onclick={() => answerConfirm(false)}>취소 (Esc)</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 960;
    display: grid;
    place-items: start center;
    padding: 60px 12px 0;
    background: rgb(0 0 0 / 0.45);
  }
  .sheet {
    width: min(360px, 100%);
    padding: 18px 18px 14px;
    border-radius: 12px;
    background: rgb(40 40 58 / 0.98);
    box-shadow:
      inset 0 0 0 0.5px rgb(255 255 255 / 0.16),
      0 12px 32px rgb(0 0 0 / 0.55);
  }
  .title {
    font-size: 13px;
    font-weight: 600;
  }
  .text {
    margin-top: 6px;
    font-size: 12px;
    line-height: 1.5;
    color: var(--fg2);
  }
  .list {
    margin: 12px 0 0;
    padding: 10px;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 6px;
    max-height: 180px;
    overflow-y: auto;
    border-radius: 8px;
    background: rgb(255 255 255 / 0.06);
  }
  .list li {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
  }
  .dot {
    flex: none;
    width: 8px;
    height: 8px;
    border-radius: 50%;
  }
  .name {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .kind {
    flex: none;
    color: var(--fg2);
  }
  .note {
    margin-top: 8px;
    font-size: 11px;
    color: var(--fg2);
  }
  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 16px;
  }
  .buttons button {
    height: 26px;
    padding: 0 14px;
    border-radius: 7px;
    font-size: 12px;
    font-weight: 600;
    background: rgb(255 255 255 / 0.1);
  }
  .buttons button:hover {
    background: rgb(255 255 255 / 0.16);
  }
  .buttons button.primary {
    background: rgb(var(--accent) / 0.85);
    color: #fff;
  }
  .buttons button.primary:hover {
    background: rgb(var(--accent));
  }
</style>
