<script lang="ts">
  // 설정 > 업데이트 — 버전·새 버전 확인과 설치 · 자동으로 업데이트 확인 (mac 업데이트 묶음)
  import { board } from "$lib/board/store.svelte";
  import { checkUpdate, openChangelog, runUpdate, setAutoUpdateCheck } from "$lib/backend";
  import ButtonControl from "../controls/ButtonControl.svelte";
  import CardSection from "../controls/CardSection.svelte";
  import SwitchControl from "../controls/SwitchControl.svelte";
  import SettingsCard from "../SettingsCard.svelte";
  import { UPDATE_ITEMS } from "./updateItems";

  const title = (id: string) => UPDATE_ITEMS.find((item) => item.id === id)?.title ?? id;
  const snap = $derived(board.snap);
  const updater = $derived(snap?.updater);
  const versionText = $derived(updater ? `Claude Board ${updater.dev ? "dev" : updater.current}` : title("version"));
  const detail = $derived(
    updater?.latest ? `새 버전 ${updater.latest} 이 나왔습니다` : (updater?.notice?.text ?? "업데이트 확인을 누르면 지금 바로 확인합니다"),
  );
</script>

{#if snap && updater}
  <div class="group" data-testid="settings-group-update">
    <CardSection title="버전">
      <div data-settings-item="version">
        <SettingsCard title={versionText} {detail}>
          <div class="buttons">
            <ButtonControl link text="바뀐 점" onclick={openChangelog} />
            {#if updater.latest}
              <ButtonControl prominent text={updater.running ? "설치하는 중…" : "업데이트"} disabled={updater.running} onclick={runUpdate} />
            {:else}
              <ButtonControl text={updater.checking ? "확인하는 중…" : "업데이트 확인"} disabled={updater.checking} onclick={() => checkUpdate(true)} />
            {/if}
          </div>
        </SettingsCard>
      </div>
      <div data-settings-item="autoUpdate">
        <SettingsCard title={title("autoUpdate")} detail="1시간마다 새 버전이 있는지 보고 보드 위에 알립니다. 설치는 직접 누를 때만 합니다">
          <SwitchControl on={snap.settings.autoUpdateCheck} label={title("autoUpdate")} onchange={setAutoUpdateCheck} />
        </SettingsCard>
      </div>
    </CardSection>
  </div>
{/if}

<style>
  .group {
    padding-block: 20px;
  }
  .buttons {
    display: flex;
    align-items: center;
    gap: 8px;
  }
</style>
