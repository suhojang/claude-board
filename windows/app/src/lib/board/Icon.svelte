<script lang="ts" module>
  export type IconName =
    | "chevron-right" | "chevron-down" | "chevron-left"
    | "folder" | "star" | "star-fill" | "xmark" | "xmark-circle" | "ellipsis"
    | "bell" | "bell-slash" | "refresh" | "compress" | "expand"
    | "pin" | "pin-slash" | "person" | "person-plus" | "person-switch" | "swap"
    | "terminal" | "terminal-outline" | "moon-stars" | "people" | "wrench" | "brain"
    | "hand" | "check-circle" | "pause-circle" | "minus-circle" | "trash" | "gauge"
    | "warn" | "arrow-down-circle" | "hammer" | "moon-zzz" | "half-circle"
    | "snap-right" | "shrink" | "grow" | "win-min" | "win-close" | "shells" | "seal-check"
    | "app-forward" | "app-plus" | "nosign" | "plus" | "panel-left" | "panel-right";
</script>

<script lang="ts">
  // macOS 판이 쓰는 SF Symbols 를 직접 그린 SVG — 크기는 글자 크기(size)에 맞춘다
  // pad: 좌우 여백(px, 음수 가능) — SF Symbol 은 보이는 그림과 칸 폭이 달라 칸 폭을 맞출 때 쓴다
  let { name, size = 12, pad = 0, class: cls = "" }: { name: IconName; size?: number; pad?: number; class?: string } = $props();
  // 속을 비우는 아이콘(채운 원 안의 체크 등)은 가리개(mask)를 쓰므로 인스턴스마다 다른 id
  const uid = $props.id();
  const m = `url(#${uid})`;
</script>

<svg class="icon {cls}" width={size} height={size} viewBox="0 0 16 16" aria-hidden="true" style:margin-inline={pad ? `${pad}px` : undefined}>
  {#if name === "chevron-right"}
    <path d="M6 3.2 10.8 8 6 12.8" fill="none" stroke="currentColor" stroke-width="2.1" stroke-linecap="round" stroke-linejoin="round" />
  {:else if name === "chevron-down"}
    <path d="M3.2 6 8 10.8 12.8 6" fill="none" stroke="currentColor" stroke-width="2.1" stroke-linecap="round" stroke-linejoin="round" />
  {:else if name === "chevron-left"}
    <path d="M10 3.2 5.2 8 10 12.8" fill="none" stroke="currentColor" stroke-width="2.3" stroke-linecap="round" stroke-linejoin="round" />
  {:else if name === "folder"}
    <path d="M1.5 4.2c0-.9.7-1.6 1.6-1.6h3.1l1.5 1.5h5.2c.9 0 1.6.7 1.6 1.6v6.4c0 .9-.7 1.6-1.6 1.6H3.1c-.9 0-1.6-.7-1.6-1.6z" fill="currentColor" />
    <path d="M1.5 6.3h13" stroke="rgb(0 0 0 / 0.18)" stroke-width="0.8" />
  {:else if name === "star" || name === "star-fill"}
    <path
      d="M8 1.6l1.9 4 4.3.5-3.2 3 .9 4.3L8 11.2l-3.9 2.2.9-4.3-3.2-3 4.3-.5z"
      fill={name === "star-fill" ? "currentColor" : "none"}
      stroke="currentColor"
      stroke-width="1.4"
      stroke-linejoin="round"
    />
  {:else if name === "xmark"}
    <path d="M4 4l8 8M12 4l-8 8" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" />
  {:else if name === "xmark-circle" || name === "check-circle" || name === "pause-circle" || name === "minus-circle" || name === "arrow-down-circle" || name === "person"}
    <mask id={uid}>
      <rect width="16" height="16" fill="#fff" />
      {#if name === "xmark-circle"}
        <path d="M5.6 5.6l4.8 4.8M10.4 5.6l-4.8 4.8" stroke="#000" stroke-width="1.7" stroke-linecap="round" />
      {:else if name === "check-circle"}
        <path d="M4.9 8.3l2.1 2.1 4.1-4.6" fill="none" stroke="#000" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" />
      {:else if name === "pause-circle"}
        <path d="M6.4 5.2v5.6M9.6 5.2v5.6" stroke="#000" stroke-width="1.8" stroke-linecap="round" />
      {:else if name === "minus-circle"}
        <path d="M4.8 8h6.4" stroke="#000" stroke-width="1.8" stroke-linecap="round" />
      {:else if name === "arrow-down-circle"}
        <path d="M8 4.6v6.2M5.4 8.4 8 11l2.6-2.6" fill="none" stroke="#000" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" />
      {:else}
        <circle cx="8" cy="6.2" r="2.4" fill="#000" />
        <path d="M3.4 12.6c1-1.9 2.7-2.8 4.6-2.8s3.6.9 4.6 2.8" fill="#000" />
      {/if}
    </mask>
    <circle cx="8" cy="8" r="7" fill="currentColor" mask={m} />
  {:else if name === "ellipsis"}
    <circle cx="3.5" cy="8" r="1.45" fill="currentColor" /><circle cx="8" cy="8" r="1.45" fill="currentColor" /><circle cx="12.5" cy="8" r="1.45" fill="currentColor" />
  {:else if name === "bell" || name === "bell-slash"}
    <path d="M8 1.8c-2.4 0-4 1.8-4 4.2v2.7L2.6 10.9c-.3.4 0 .9.5.9h9.8c.5 0 .8-.5.5-.9L12 8.7V6c0-2.4-1.6-4.2-4-4.2z" fill="currentColor" />
    <path d="M6.3 12.8a1.8 1.8 0 0 0 3.4 0z" fill="currentColor" />
    {#if name === "bell-slash"}
      <path d="M2.2 2.2l11.6 11.6" stroke="var(--icon-cut, rgb(20 20 30))" stroke-width="2.6" stroke-linecap="round" />
      <path d="M2.2 2.2l11.6 11.6" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" />
    {/if}
  {:else if name === "refresh"}
    <path d="M12.6 6.2A4.9 4.9 0 0 0 3.6 5" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" />
    <path d="M13.6 3.2v3.6H10z" fill="currentColor" stroke="currentColor" stroke-width="0.6" stroke-linejoin="round" />
    <path d="M3.4 9.8a4.9 4.9 0 0 0 9 1.2" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" />
    <path d="M2.4 12.8V9.2H6z" fill="currentColor" stroke="currentColor" stroke-width="0.6" stroke-linejoin="round" />
  {:else if name === "compress"}
    <!-- rectangle.compress.vertical — 가운데 납작한 사각형, 위아래에서 안쪽으로 향한 화살표 -->
    <rect x="2.2" y="6.3" width="11.6" height="3.4" rx="1.1" fill="none" stroke="currentColor" stroke-width="1.4" />
    <path d="M8 1.2v3.4M6.1 2.9 8 4.8l1.9-1.9M8 14.8v-3.4M6.1 13.1 8 11.2l1.9 1.9" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" />
  {:else if name === "expand"}
    <!-- rectangle.expand.vertical — 가운데 사각형, 위아래로 바깥을 향한 화살표 -->
    <rect x="2.2" y="5.2" width="11.6" height="5.6" rx="1.4" fill="none" stroke="currentColor" stroke-width="1.4" />
    <path d="M8 4.2V1.2M6.1 3 8 1.1 9.9 3M8 11.8v3M6.1 13 8 14.9 9.9 13" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" />
  {:else if name === "pin" || name === "pin-slash"}
    <g transform={name === "pin-slash" ? "rotate(45 8 8)" : ""}>
      <path d="M5.2 1.6h5.6l-.7 1.3v3.2l2 2.4v1H3.9v-1l2-2.4V2.9z" fill={name === "pin" ? "currentColor" : "none"} stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" />
      <path d="M8 9.5v5" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" />
    </g>
    {#if name === "pin-slash"}<path d="M2.5 2.5l11 11" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" />{/if}
  {:else if name === "person-plus"}
    <circle cx="6.5" cy="5" r="2.6" fill="currentColor" />
    <path d="M1.5 13.5c.6-2.7 2.6-4.2 5-4.2s4.4 1.5 5 4.2z" fill="currentColor" />
    <path d="M12.8 2.5v5M10.3 5h5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
  {:else if name === "person-switch"}
    <path d="M13 6A5.2 5.2 0 0 0 3.4 4.4M3 10a5.2 5.2 0 0 0 9.6 1.6" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" />
    <path d="M14 2.6v3.8h-3.8zM2 13.4V9.6h3.8z" fill="currentColor" />
  {:else if name === "swap"}
    <path d="M2.5 5h10M10 2.5 12.5 5 10 7.5M13.5 11h-10M6 8.5 3.5 11 6 13.5" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" />
  {:else if name === "terminal"}
    <mask id={uid}>
      <rect width="16" height="16" fill="#fff" />
      <path d="M4.2 5.8 6.4 8 4.2 10.2M7.8 10.6h4" fill="none" stroke="#000" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" />
    </mask>
    <rect x="1.2" y="2.2" width="13.6" height="11.6" rx="2.4" fill="currentColor" mask={m} />
  {:else if name === "terminal-outline"}
    <rect x="1.6" y="2.6" width="12.8" height="10.8" rx="2.2" fill="none" stroke="currentColor" stroke-width="1.3" />
    <path d="M4.4 6 6.4 8 4.4 10M7.8 10.2h3.6" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" />
  {:else if name === "moon-stars"}
    <path d="M7.2 2.2a5.6 5.6 0 1 0 6.6 7.6A5 5 0 0 1 7.2 2.2z" fill="currentColor" />
    <path d="M12 1.8l.5 1.2 1.2.5-1.2.5-.5 1.2-.5-1.2-1.2-.5 1.2-.5zM14.3 5.6l.3.7.7.3-.7.3-.3.7-.3-.7-.7-.3.7-.3z" fill="currentColor" />
  {:else if name === "moon-zzz"}
    <path d="M6.6 3a5.6 5.6 0 1 0 6.6 7.6A5 5 0 0 1 6.6 3z" fill="currentColor" />
    <path d="M10 1.6h3.2L10 5h3.2" fill="none" stroke="currentColor" stroke-width="1.1" stroke-linecap="round" stroke-linejoin="round" />
  {:else if name === "people"}
    <circle cx="5.6" cy="5.2" r="2.5" fill="currentColor" />
    <path d="M1 13.4c.4-2.6 2.2-4 4.6-4s4.2 1.4 4.6 4z" fill="currentColor" />
    <circle cx="11.3" cy="5.6" r="2.1" fill="currentColor" />
    <path d="M10.9 9.4c2.2 0 3.7 1.3 4.1 3.6h-3.6c-.2-1.4-.6-2.6-1.4-3.5z" fill="currentColor" />
  {:else if name === "wrench"}
    <!-- wrench.and.screwdriver.fill — 몽키와 드라이버가 엇갈린다 -->
    <path d="M11.6 1.4a3.2 3.2 0 0 0-3.1 4.1L2.4 11.6a1.4 1.4 0 0 0 2 2l6.1-6.1a3.2 3.2 0 0 0 4.1-3.1l-1.9 1.4-1.7-.3-.3-1.7z" fill="currentColor" />
    <path d="M3 2.2l1.3-.7 2.1 2.1-.7 1.3-1.5.3L2.4 3.7z M7.6 8.4l1.2-1.2 5.4 5.4a1 1 0 0 1-1.4 1.4z" fill="currentColor" />
  {:else if name === "brain"}
    <path d="M7.8 1.8c-3 0-5.2 2.1-5.2 4.9 0 1.4.5 2.4 1.3 3.3v3.6c0 .4.3.6.6.6h4.2c.3 0 .6-.3.6-.6v-1.4h1.6c.7 0 1.2-.5 1.2-1.2V9.3l1.1-.4c.3-.1.4-.5.2-.8l-1.3-2C12 3.7 10.3 1.8 7.8 1.8z" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round" />
    <path d="M5.4 6.6c0-1.1.9-1.8 1.8-1.8.4-.5 1-.7 1.6-.5M6.6 8.6c.5.4 1.3.4 1.8 0" fill="none" stroke="currentColor" stroke-width="1.1" stroke-linecap="round" />
  {:else if name === "hand"}
    <path
      d="M5.3 8.2V3.4a.95.95 0 0 1 1.9 0v3.8V2.5a.95.95 0 0 1 1.9 0v4.7V3.2a.95.95 0 0 1 1.9 0v4.6-2.4a.9.9 0 0 1 1.8 0v4.4c0 2.8-1.9 4.7-4.5 4.7-1.7 0-2.8-.7-3.8-2L2.6 9.5a1 1 0 0 1 1.5-1.3z"
      fill="currentColor"
    />
  {:else if name === "trash"}
    <path d="M2.6 4h10.8M6 4V2.6h4V4M4 4l.7 9.4h6.6L12 4" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" />
  {:else if name === "gauge"}
    <path d="M2.2 11.4a6 6 0 1 1 11.6 0" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
    <path d="M8 10.2 5.4 6.4" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" />
    <circle cx="8" cy="10.4" r="1.3" fill="currentColor" />
  {:else if name === "warn"}
    <mask id={uid}>
      <rect width="16" height="16" fill="#fff" />
      <path d="M8 6v3.4" stroke="#000" stroke-width="1.7" stroke-linecap="round" />
      <circle cx="8" cy="11.6" r="1" fill="#000" />
    </mask>
    <path d="M6.9 2.3a1.3 1.3 0 0 1 2.2 0l5.5 9.7c.5.9-.1 1.9-1.1 1.9H2.5c-1 0-1.6-1-1.1-1.9z" fill="currentColor" mask={m} />
  {:else if name === "hammer"}
    <path d="M2.2 4.4 6.6 1.6l3.6 2.4-1.4 1.8-1.6-.6-.8 1 6.3 6.4a1.2 1.2 0 0 1-1.7 1.7L4.6 8l-1 .9z" fill="currentColor" />
  {:else if name === "half-circle"}
    <circle cx="8" cy="8" r="6.2" fill="none" stroke="currentColor" stroke-width="1.5" />
    <path d="M8 1.8a6.2 6.2 0 0 0 0 12.4z" fill="currentColor" />
  {:else if name === "plus"}
    <path d="M8 2.6v10.8M2.6 8h10.8" stroke="currentColor" stroke-width="2" stroke-linecap="round" />
  {:else if name === "panel-left"}
    <rect x="1.2" y="2.4" width="13.6" height="11.2" rx="2.4" fill="none" stroke="currentColor" stroke-width="1.4" />
    <rect x="3.4" y="4.6" width="4.2" height="6.8" rx="1" fill="currentColor" />
  {:else if name === "panel-right"}
    <rect x="1.2" y="2.4" width="13.6" height="11.2" rx="2.4" fill="none" stroke="currentColor" stroke-width="1.4" />
    <rect x="8.4" y="4.6" width="4.2" height="6.8" rx="1" fill="currentColor" />
  {:else if name === "snap-right"}
    <rect x="1.2" y="2.4" width="13.6" height="11.2" rx="2.4" fill="none" stroke="currentColor" stroke-width="1.4" />
    <rect x="8.4" y="4.6" width="4.2" height="6.8" rx="1" fill="currentColor" />
  {:else if name === "shrink"}
    <path d="M14 2 9.4 6.6M9.2 3v3.8H13M2 14l4.6-4.6M3 9.2h3.8V13" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" />
  {:else if name === "grow"}
    <path d="M9.4 6.6 14 2M10.2 2H14v3.8M6.6 9.4 2 14M2 10.2V14h3.8" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" />
  {:else if name === "win-min"}
    <path d="M3 8h10" stroke="currentColor" stroke-width="1" />
  {:else if name === "win-close"}
    <path d="M3.5 3.5l9 9M12.5 3.5l-9 9" stroke="currentColor" stroke-width="1" />
  {:else if name === "shells"}
    <rect x="3.6" y="1.6" width="10.8" height="8.4" rx="1.8" fill="none" stroke="currentColor" stroke-width="1.2" />
    <rect x="1.4" y="5.4" width="10.4" height="8.8" rx="1.8" fill="currentColor" />
    <path d="M3.6 8.6 5.2 10l-1.6 1.4M6.4 11.8h2.8" fill="none" stroke="rgb(20 20 30)" stroke-width="1.1" stroke-linecap="round" stroke-linejoin="round" />
  {:else if name === "seal-check"}
    <mask id={uid}>
      <rect width="16" height="16" fill="#fff" />
      <path d="M5.2 8.2l1.9 1.9 3.8-4.2" fill="none" stroke="#000" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" />
    </mask>
    <path
      d="M8 .9l1.6 1.2 2-.1.6 1.9 1.7 1.1-.6 1.9.6 1.9-1.7 1.1-.6 1.9-2-.1L8 15.1l-1.6-1.2-2 .1-.6-1.9-1.7-1.1.6-1.9-.6-1.9 1.7-1.1.6-1.9 2 .1z"
      fill="currentColor"
      stroke="currentColor"
      stroke-width="0.8"
      stroke-linejoin="round"
      mask={m}
    />
  {:else if name === "app-forward"}
    <rect x="1.6" y="1.6" width="12.8" height="12.8" rx="3" fill="none" stroke="currentColor" stroke-width="1.3" />
    <path d="M6 10 10.4 5.6M7 5.4h3.6V9" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" />
  {:else if name === "app-plus"}
    <rect x="1.6" y="1.6" width="12.8" height="12.8" rx="3" fill="none" stroke="currentColor" stroke-width="1.3" />
    <path d="M8 5v6M5 8h6" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" />
  {:else if name === "nosign"}
    <circle cx="8" cy="8" r="6" fill="none" stroke="currentColor" stroke-width="1.5" />
    <path d="M3.8 3.8l8.4 8.4" stroke="currentColor" stroke-width="1.5" />
  {/if}
</svg>

<style>
  .icon {
    display: inline-block;
    flex: none;
    vertical-align: middle;
    overflow: visible;
  }
</style>
