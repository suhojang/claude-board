<script lang="ts">
  // 설정 > 창 — 보드 위치 · 투명도 · 설정 화면 전환 (mac 창 묶음. 펼치기·접기 애니메이션은 3단계)
  import { board } from "$lib/board/store.svelte";
  import { win } from "$lib/board/pointer.svelte";
  import { setBoardSide, setOpacity, setSettingsTransition } from "$lib/backend";
  import type { BoardSide, SettingsTransition } from "$lib/types";
  import CardSection from "../controls/CardSection.svelte";
  import SegmentsControl from "../controls/SegmentsControl.svelte";
  import SettingsCard from "../SettingsCard.svelte";
  import { WINDOW_ITEMS } from "./windowItems";

  const title = (id: string) => WINDOW_ITEMS.find((item) => item.id === id)?.title ?? id;
  const settings = $derived(board.snap?.settings);

  const SIDES: { value: BoardSide; text: string }[] = [
    { value: "right", text: "오른쪽" },
    { value: "left", text: "왼쪽" },
  ];
  const TRANSITIONS: { value: SettingsTransition; text: string }[] = [
    { value: "slide", text: "옆으로 밀기" },
    { value: "fade", text: "서서히 바뀌기" },
    { value: "none", text: "효과 없음" },
  ];

  // 투명도 — 끄는 동안과 놓은 뒤 1.2초는 보드 창에 고른 투명도를 미리 보여 준다(보드 윗줄의 슬라이더와 같다)
  const level = $derived(win.dragging ? win.dragLevel : (settings?.opacity ?? 1));
  let settle: ReturnType<typeof setTimeout> | undefined;

  function startPreview() {
    clearTimeout(settle);
    win.dragLevel = settings?.opacity ?? 1;
    win.dragging = true;
    win.previewing = true;
  }

  function endPreview() {
    if (!win.dragging) return;
    setOpacity(Math.round(win.dragLevel * 100) / 100);
    win.dragging = false;
    settle = setTimeout(() => (win.previewing = false), 1200);
  }
</script>

{#if settings}
  <div class="group" data-testid="settings-group-window">
    <CardSection title="배치">
      <div data-settings-item="side">
        <SettingsCard title={title("side")} detail="펼치면 터미널 반대쪽, 접으면 화면 그쪽 끝에 붙습니다">
          <SegmentsControl options={SIDES} selected={settings.boardSide} label={title("side")} onpick={setBoardSide} />
        </SettingsCard>
      </div>
      <div data-settings-item="opacity">
        <SettingsCard title={title("opacity")} detail="마우스가 보드 창 밖에 있을 때의 투명도. 화면 전체를 덮은 창은 늘 불투명합니다">
          <div class="opacity">
            <input
              type="range"
              min="0.3"
              max="1"
              step="0.01"
              value={level}
              aria-label={title("opacity")}
              onpointerdown={startPreview}
              oninput={(e) => {
                if (!win.dragging) startPreview();
                win.dragLevel = Number(e.currentTarget.value);
              }}
              onpointerup={endPreview}
              onchange={endPreview}
              onkeyup={endPreview}
            />
            <span class="percent num">{Math.round(level * 100)}%</span>
          </div>
        </SettingsCard>
      </div>
    </CardSection>
    <CardSection title="움직임">
      <div data-settings-item="transition">
        <SettingsCard title={title("transition")} detail="터미널 영역이 설정 화면으로 바뀔 때의 효과">
          <SegmentsControl options={TRANSITIONS} selected={settings.settingsTransition} label={title("transition")} onpick={setSettingsTransition} />
        </SettingsCard>
      </div>
    </CardSection>
  </div>
{/if}

<style>
  .group {
    padding-block: 20px;
  }
  .opacity {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .opacity input {
    width: 150px;
    accent-color: rgb(var(--accent));
    cursor: pointer;
  }
  .percent {
    width: 40px;
    text-align: right;
    font-size: 12px;
    font-weight: 600;
  }
</style>
