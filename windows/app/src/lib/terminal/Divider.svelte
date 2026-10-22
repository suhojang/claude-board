<script lang="ts">
  // 터미널 칸과 보드 칸 사이 경계 — 끌면 보드 칸 폭이 바뀐다. 끄는 동안은 onresize 로 폭을 알리고, 놓을 때 oncommit 으로 한 번 알린다.
  // root 는 두 칸을 담은 상자, side 는 보드가 있는 쪽이다
  import type { BoardSide } from "$lib/types";

  let {
    root,
    side,
    min,
    max,
    onresize,
    oncommit,
  }: {
    root: HTMLElement | undefined;
    side: BoardSide;
    min: number;
    max: number;
    onresize: (width: number) => void;
    oncommit: (width: number) => void;
  } = $props();

  let dragging = $state(false);
  let last = 0;

  // 포인터 위치에서 보드 칸 폭을 구한다. 보드가 오른쪽이면 상자 오른쪽 끝에서, 왼쪽이면 왼쪽 끝에서 잰 거리다
  function widthAt(clientX: number) {
    const box = root!.getBoundingClientRect();
    const width = side === "right" ? box.right - clientX : clientX - box.left;
    return Math.min(Math.max(width, min), Math.max(min, max));
  }

  function down(e: PointerEvent) {
    if (!root || e.button !== 0) return;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    dragging = true;
    last = widthAt(e.clientX);
    onresize(last);
    e.preventDefault();
  }

  function move(e: PointerEvent) {
    if (!dragging) return;
    last = widthAt(e.clientX);
    onresize(last);
  }

  function up() {
    if (!dragging) return;
    dragging = false;
    oncommit(last);
  }
</script>

<div
  class="divider"
  class:dragging
  role="separator"
  aria-orientation="vertical"
  aria-label="터미널과 보드 사이 경계"
  onpointerdown={down}
  onpointermove={move}
  onpointerup={up}
  onpointercancel={up}
>
  <span class="grip"></span>
</div>

<style>
  .divider {
    flex: none;
    width: 8px;
    display: grid;
    place-items: center;
    cursor: col-resize;
    background: var(--term-edge);
    touch-action: none;
  }
  .grip {
    width: 3px;
    height: 44px;
    border-radius: 1.5px;
    background: #5c5c5c;
  }
  .divider.dragging .grip {
    background: #7aa3f7;
  }
</style>
