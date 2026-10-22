<script lang="ts">
  // 창 최소화·닫기 단추 — 보드 제목 막대 오른쪽 끝에 있고, 보드가 왼쪽이면 터미널을 펼친 동안 터미널 제목 띠 오른쪽 끝에 놓인다(창의 오른쪽 끝)
  import { hideBoard, minimizeBoard } from "$lib/backend";
  import Icon from "$lib/board/Icon.svelte";
  import { hint } from "$lib/board/hint.svelte";

  let { round = false }: { round?: boolean } = $props();
</script>

<button class="win" onclick={minimizeBoard} use:hint={"최소화"} aria-label="최소화"><Icon name="win-min" size={10} /></button>
<button class="win close" class:round onclick={hideBoard} use:hint={"닫기 — 보드는 알림 영역 아이콘에 남습니다"} aria-label="닫기">
  <Icon name="win-close" size={10} />
</button>

<style>
  .win {
    flex: none;
    width: 40px;
    height: 32px;
    display: grid;
    place-items: center;
    color: rgb(255 255 255 / 0.75);
    transition: background 0.12s;
  }
  .win:hover {
    color: #fff;
    background: rgb(255 255 255 / 0.1);
  }
  .win.close.round {
    border-top-right-radius: 10px;
  }
  .win.close:hover {
    background: #c42b1c;
  }
</style>
