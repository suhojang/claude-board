<script lang="ts" module>
  // 가운데를 줄이는 글자(SwiftUI truncationMode(.middle)) — 긴 글이 넘치면 앞 30%·뒤 70% 글자만 남기고 가운데를 … 로 바꾼다.
  // 남는 폭에서 20px 를 비운다(macOS 가 칸을 꽉 채우지 않고 남기는 만큼)
  const SLACK = 20;
  let canvas: CanvasRenderingContext2D | null = null;

  function middle(node: HTMLElement, full: string) {
    let text = full;
    function fit() {
      canvas ??= document.createElement("canvas").getContext("2d");
      const ctx = canvas;
      const avail = node.clientWidth - SLACK;
      if (!ctx || avail <= 0) {
        node.textContent = text;
        return;
      }
      const cs = getComputedStyle(node);
      ctx.font = `${cs.fontWeight} ${cs.fontSize} ${cs.fontFamily}`;
      ctx.letterSpacing = cs.letterSpacing === "normal" ? "0px" : cs.letterSpacing;
      const chars = Array.from(text);
      const cut = (k: number) => {
        const h = Math.round(k * 0.3);
        return chars.slice(0, h).join("") + "…" + chars.slice(chars.length - (k - h)).join("");
      };
      let out = text;
      if (ctx.measureText(text).width > node.clientWidth) {
        let k = chars.length - 1;
        while (k > 1 && ctx.measureText(cut(k)).width > avail) k--;
        out = cut(k);
      }
      node.textContent = out;
    }
    const ro = new ResizeObserver(fit);
    ro.observe(node);
    fit();
    return {
      update(next: string) {
        text = next;
        fit();
      },
      destroy: () => ro.disconnect(),
    };
  }
</script>

<script lang="ts">
  // 새 버전 알림 줄(ClaudeBoard.swift:4186-4224) — 전체·중간 화면의 윗줄 아래에 뜬다.
  // 새 버전이 없으면 손으로 확인한 결과(최신·확인 실패·개발용 빌드)만 단추 없이 잠깐 보인다
  import type { Snapshot } from "$lib/types";
  import { openChangelog, runUpdate } from "$lib/backend";
  import Icon, { type IconName } from "./Icon.svelte";
  import { hint } from "./hint.svelte";

  let { updater }: { updater: Snapshot["updater"] } = $props();

  const notice = $derived(updater.latest == null ? updater.notice : null);
  const tint = $derived(updater.failed ? "255 168 92" : "217 120 87");
  const icon = $derived<IconName>(
    notice ? ({ ok: "check-circle", warn: "warn", dev: "hammer" } as const)[notice.kind] : updater.failed ? "warn" : "arrow-down-circle",
  );
  const text = $derived(
    notice?.text ??
      (updater.failed
        ? "업데이트하지 못했습니다 · ~/.claude/notifier/update.log"
        : updater.running
          ? "업데이트 중… 끝나면 보드가 다시 열립니다"
          : `새 버전 ${updater.latest ?? ""} 이 나왔습니다`),
  );
</script>

<div class="banner" style:--t={tint}>
  <span class="ic"><Icon name={icon} size={14.5} /></span>
  <span class="text" use:middle={text}></span>
  {#if updater.running}
    <span class="spinner"></span>
  {:else if !notice}
    <!-- 무엇이 바뀌었는지 — 브라우저에서 GitHub 의 windows/CHANGELOG.md 를 연다 -->
    <button class="link" onclick={openChangelog} use:hint={"GitHub 에서 CHANGELOG.md 를 엽니다"}>변경 내용</button>
    <span class="sep"></span>
    <button class="link go" onclick={runUpdate} use:hint={"새 버전을 받아 바꿔 끼웁니다 — 끝나면 보드가 다시 열립니다"}>
      {updater.failed ? "다시 시도" : "업데이트"}
    </button>
  {/if}
</div>

<style>
  .banner {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 10px;
    border-radius: 8px;
    line-height: 15px;
    background: rgb(var(--t) / 0.12);
  }
  .ic {
    display: grid;
    place-items: center;
    color: rgb(var(--t));
  }
  .text {
    flex: 1;
    min-width: 0;
    font-size: 12px;
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
  }
  .link {
    flex: none;
    font-size: 12px;
    font-weight: 600;
    color: var(--fg2);
  }
  .link:hover {
    color: var(--fg);
  }
  .link.go {
    color: rgb(var(--t) / 0.85);
  }
  .link.go:hover {
    color: rgb(var(--t));
  }
  .sep {
    width: 0.5px;
    height: 10px;
    margin: 0 2px;
    background: rgb(255 255 255 / 0.14);
  }
  .spinner {
    flex: none;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    border: 2px solid rgb(255 255 255 / 0.15);
    border-top-color: rgb(255 255 255 / 0.7);
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
