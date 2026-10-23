<script lang="ts">
  // 경로 카드 머리의 작은 아이콘 단추(상단 고정·목록에서 지우기) — 마우스를 올리면 커져서 누르기 쉽다
  import Icon, { type IconName } from "./Icon.svelte";
  import { hint } from "./hint.svelte";

  let {
    icon,
    tint = null,
    help,
    onclick,
  }: { icon: IconName; tint?: string | null; help: string; onclick: () => void } = $props();
  // SF Symbol 의 보이는 크기에 맞춘 아이콘 크기(16 칸 도안 안의 여백 때문에 글자 크기와 다르다)
  const SIZE: Partial<Record<IconName, number>> = { plus: 14, star: 17, "star-fill": 17, xmark: 17 };
</script>

<button
  class="hb"
  class:tinted={tint != null}
  style:--t={tint}
  onclick={(e) => {
    e.stopPropagation();
    onclick();
  }}
  ondblclick={(e) => e.stopPropagation()}
  use:hint={help}
>
  <Icon name={icon} size={SIZE[icon] ?? 12} />
</button>

<style>
  .hb {
    flex: none;
    width: 26px;
    height: 26px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    color: rgb(255 255 255 / 0.55);
    background: rgb(255 255 255 / 0.06);
    transition:
      transform 0.25s cubic-bezier(0.3, 1.6, 0.5, 1),
      background 0.15s,
      color 0.15s;
  }
  .hb:hover {
    transform: scale(1.3);
    color: rgb(255 255 255 / 0.95);
    background: rgb(255 255 255 / 0.16);
  }
  .hb.tinted {
    color: rgb(var(--t));
  }
</style>
