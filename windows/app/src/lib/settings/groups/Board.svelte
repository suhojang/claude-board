<script lang="ts">
  // 설정 > 보드 — 사용 한도 간략히 보기 (mac 보드 묶음. 글자 크기는 3단계)
  import { board } from "$lib/board/store.svelte";
  import { setUsageCompact } from "$lib/backend";
  import CardSection from "../controls/CardSection.svelte";
  import SwitchControl from "../controls/SwitchControl.svelte";
  import SettingsCard from "../SettingsCard.svelte";
  import { BOARD_ITEMS } from "./boardItems";

  const title = (id: string) => BOARD_ITEMS.find((item) => item.id === id)?.title ?? id;
  const settings = $derived(board.snap?.settings);
</script>

{#if settings}
  <div class="group" data-testid="settings-group-board">
    <CardSection title="보드">
      <div data-settings-item="usageCompact">
        <SettingsCard title={title("usageCompact")} detail="5시간·주간 한도 칸을 한 줄로 줄입니다">
          <SwitchControl on={settings.usageCompact} label={title("usageCompact")} onchange={setUsageCompact} />
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
