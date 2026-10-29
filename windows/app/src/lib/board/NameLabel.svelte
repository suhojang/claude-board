<script lang="ts" module>
  // 24 코드포인트를 넘는 이름은 앞 11자 + … + 뒤 11자로 줄인다 — 끝의 번호(-2 등)가 보이게 한다(SessionNameLabel)
  export function shortName(name: string): string {
    const chars = Array.from(name);
    return chars.length > 24 ? `${chars.slice(0, 11).join("")}…${chars.slice(-11).join("")}` : name;
  }
</script>

<script lang="ts">
  // 세션 이름 라벨: 태그 아이콘 + Claude Code 세션 이름. 마우스를 올리면 전체 이름이 나온다
  import { hint } from "./hint.svelte";

  let { name }: { name: string } = $props();
</script>

<span class="cap" use:hint={`세션 이름 ${name}`}>
  <svg width="9" height="9" viewBox="0 0 16 16" aria-hidden="true">
    <path
      d="M1.8 2.6v5.1c0 .4.2.8.5 1.1l5.2 5.2c.6.6 1.6.6 2.2 0l4.2-4.2c.6-.6.6-1.6 0-2.2L8.7 2.4a1.6 1.6 0 0 0-1.1-.5H2.5c-.4 0-.7.3-.7.7z"
      fill="none"
      stroke="currentColor"
      stroke-width="1.6"
      stroke-linejoin="round"
    />
    <circle cx="5.2" cy="5.2" r="1.2" fill="currentColor" />
  </svg>
  {shortName(name)}
</span>

<style>
  .cap {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    flex: none;
    padding: 1.5px 6px;
    font-size: 10px;
    font-weight: 600;
    line-height: 13px;
    white-space: nowrap;
    color: rgb(255 255 255 / 0.72);
    background: rgb(255 255 255 / 0.08);
    box-shadow: inset 0 0 0 0.5px rgb(255 255 255 / 0.14);
    border-radius: 999px;
  }
</style>
