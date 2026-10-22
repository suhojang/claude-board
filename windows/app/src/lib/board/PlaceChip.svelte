<script lang="ts">
  // 세션 위치 칩(ClaudeBoard.swift PlaceChip) — 어디서 도는지: 보드 탭 CB(탭 안 claude -p 는 CB -p), 다른 터미널은 앱 이름,
  // 백그라운드는 실제 실행 방식($ claude -p·--bg), 끝난 세션은 누르면 보드에서 이어 연다는 표시.
  // Session.place 는 이후 단계(세션·탭 짝짓기)가 채운다 — 없으면 이 칩을 그리지 않는다
  import type { SessionPlace } from "$lib/types";
  import { hint } from "./hint.svelte";

  let { place, headless = false }: { place: SessionPlace; headless?: boolean } = $props();

  const view = $derived.by(() => {
    const mode = place.mode;
    switch (place.kind) {
      case "tab":
        return {
          text: mode ? `CB ${mode}` : "CB",
          bg: "46 59 87",
          fg: "168 194 255",
          mono: false,
          tip: headless || mode ? `보드 탭 안에서 시작된 claude ${mode ?? "-p"} (탭이 닫히면 함께 끝납니다)` : "보드 탭에서 도는 세션 — 누르면 그 탭으로",
        };
      case "terminal":
        return place.appName
          ? { text: place.appName, bg: "36 64 74", fg: "143 217 232", mono: false, tip: `${place.appName} 에서 도는 세션` }
          : null;
      case "background":
        return { text: `$ claude ${mode ?? "-p"}`, bg: "59 51 69", fg: "212 184 255", mono: true, tip: "터미널 없이 도는 세션입니다. 보드는 상태만 보여 줍니다" };
      case "ended":
        return headless
          ? { text: "$ claude -p", bg: "", fg: "", mono: true, tip: "끝난 백그라운드 세션입니다. 메뉴의 세션 다시 열기로 보드 탭에서 엽니다" }
          : { text: "보드에서 이어 열기", bg: "", fg: "", mono: false, tip: "누르면 보드 새 탭에서 이 대화를 이어 엽니다" };
    }
  });
</script>

{#if view}
  <span
    class="chip"
    class:mono={view.mono}
    class:muted={place.kind === "ended"}
    style:--bg={view.bg || undefined}
    style:--fg={view.fg || undefined}
    use:hint={view.tip}>{view.text}</span
  >
{/if}

<style>
  .chip {
    flex: none;
    padding: 1.5px 6px;
    border-radius: 999px;
    font-size: 10px;
    font-weight: 600;
    line-height: 13px;
    white-space: nowrap;
    color: rgb(var(--fg));
    background: rgb(var(--bg));
  }
  .chip.mono {
    font-family: var(--mono);
  }
  .chip.muted {
    color: var(--fg2);
    background: rgb(255 255 255 / 0.08);
  }
</style>
