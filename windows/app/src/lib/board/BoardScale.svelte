<script lang="ts">
  // 보드 글자 크기 배율(mac BoardFontScale) — 칸 안쪽 내용 전체에 CSS 변수 하나(--board-scale)로 `zoom` 을 건다.
  // 제목 막대·창 바탕·알림 패널은 이 칸 밖이라 그대로다. 규칙(범위·단계·기본값)은 fontScale.ts 가 소유한다.
  // zoom 은 글자뿐 아니라 이 칸의 아이콘·여백도 같이 키운다(mac 은 글자만 키우고 아이콘은 그대로다 — 차이는 SPEC 에 적는다).
  // 칸을 감싸는 바깥 칸 크기(+page.svelte 의 ResizeObserver 가 재는 보드 칸)는 화면 픽셀 그대로이고, 안쪽 레이아웃 폭·높이만 scale 로 나뉜다.
  import type { Snippet } from "svelte";
  import { normalizeBoardFontScale } from "./fontScale";

  let { scale, children }: { scale: unknown; children: Snippet } = $props();

  // 저장된 값이 깨져 있어도 규칙을 거친다 — 범위 밖·문자열·NaN 은 끝값 또는 1
  const value = $derived(normalizeBoardFontScale(scale));
</script>

<!-- 1 이면 zoom 을 아예 걸지 않는다 — 기본 배율에서 레이아웃·렌더 경로가 이 칸이 없을 때와 같다 -->
<div class="scale" class:on={value !== 1} style:--board-scale={value}>
  {@render children()}
</div>

<style>
  .scale {
    flex: 1 1 auto;
    min-height: 0;
    min-width: 0;
    display: flex;
    flex-direction: column;
    /* 자식(.content·MiniBoard)이 쓰던 세로 flex 자리를 그대로 받는다 */
  }
  .scale.on {
    zoom: var(--board-scale);
  }
</style>
