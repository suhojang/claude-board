<script lang="ts">
  // 설정 > 알림 — 모든 알림 끄기 · 알림 위치(화면 그림에서 고르기) (mac 알림 묶음)
  import { board } from "$lib/board/store.svelte";
  import { setAllMuted, setNotifyPosition } from "$lib/backend";
  import CardSection from "../controls/CardSection.svelte";
  import SwitchControl from "../controls/SwitchControl.svelte";
  import ScreenPicker from "../ScreenPicker.svelte";
  import SettingsCard from "../SettingsCard.svelte";
  import { POSITION_NAMES } from "../screenPick";
  import { NOTIFY_ITEMS } from "./notifyItems";

  const title = (id: string) => NOTIFY_ITEMS.find((item) => item.id === id)?.title ?? id;
  const snap = $derived(board.snap);
</script>

{#if snap}
  <div class="group" data-testid="settings-group-notify">
    <CardSection title="알림">
      <div data-settings-item="allMute">
        <SettingsCard
          title={title("allMute")}
          detail="켜면 모든 세션의 작업 완료·승인 대기 알림을 띄우지 않습니다. 끄면 세션마다 정해 둔 대로 돌아갑니다"
        >
          <SwitchControl on={snap.allMuted} label={title("allMute")} onchange={setAllMuted} />
        </SettingsCard>
      </div>
      <div data-settings-item="position">
        <SettingsCard title={title("position")} detail="패널이 뜨는 화면 자리. {POSITION_NAMES[snap.settings.notifyPosition]}">
          <ScreenPicker value={snap.settings.notifyPosition} onpick={setNotifyPosition} />
        </SettingsCard>
      </div>
    </CardSection>
  </div>
{/if}

<style>
  .group {
    padding-block: 20px;
  }
</style>
