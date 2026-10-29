<script lang="ts">
  // 설정 항목 한 줄을 담는 카드(제목·설명·오른쪽 조작) — 묶음 화면들이 이 카드로 줄을 그린다.
  // block 이면 조작이 줄 전체 폭을 쓴다(캐릭터 고르기·화면 그림처럼 넓은 것). 설정 화면이 좁으면(컨테이너 720px 이하) 조작이 설명 아래로 내려간다.
  import type { Snippet } from "svelte";

  let { title, detail, block = false, children }: { title: string; detail?: string; block?: boolean; children?: Snippet } = $props();
</script>

<div class="card" class:block data-testid="settings-card" data-title={title} role="group" aria-label={title}>
  <div class="text">
    <div class="title">{title}</div>
    {#if detail}<div class="detail">{detail}</div>{/if}
  </div>
  {#if children}<div class="control">{@render children()}</div>{/if}
</div>

<style>
  .card {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 12px 14px;
    border-radius: 10px;
    background: rgb(255 255 255 / 0.035);
    border: 1px solid rgb(255 255 255 / 0.07);
  }
  .card + :global(.card) {
    margin-top: 10px;
  }
  .text {
    flex: 1 1 auto;
    min-width: 0;
  }
  .title {
    font-size: 13px;
    font-weight: 600;
    color: var(--fg);
  }
  .detail {
    margin-top: 3px;
    font-size: 11.5px;
    line-height: 1.4;
    color: var(--fg2);
  }
  .control {
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .block {
    flex-direction: column;
    align-items: stretch;
    gap: 10px;
  }
  .block .control {
    flex: 1 1 auto;
    flex-wrap: wrap;
  }
  @container settings (max-width: 720px) {
    .card {
      flex-direction: column;
      align-items: stretch;
      gap: 10px;
    }
    .control {
      flex-wrap: wrap;
    }
  }
</style>
