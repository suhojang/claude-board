<script lang="ts">
  // 설정 > 단축키 — 줄의 키 칸을 누르고 새 키를 누르면 바뀐다(Esc 취소, Backspace 비우기). 바꾸면 메뉴·탭 줄·제목 막대의 안내 글자와
  // 전역 단축키(Ctrl+Alt+B/T)가 바로 따라간다. 보드 안에서 같은 키를 쓰는 동작이 있으면 그 줄에서 알리고 상대를 비울 수 있다.
  import { hint } from "$lib/board/hint.svelte";
  import { SHORTCUT_ACTIONS, actionOf } from "$lib/shortcuts/actions";
  import { label } from "$lib/shortcuts/keyCombo";
  import { combo, conflicts } from "$lib/shortcuts/rules";
  import {
    clearShortcut,
    connectShortcutSettings,
    currentOverrides,
    resetAllShortcuts,
    resetShortcut,
    ShortcutSettings,
    startRecording,
    stopRecording,
  } from "$lib/shortcuts/store.svelte";
  import SettingsCard from "../SettingsCard.svelte";

  $effect(() => connectShortcutSettings());

  const overrides = $derived(currentOverrides());
  const changedAny = $derived(Object.keys(overrides).length > 0);
  // 묶음 이름은 표에 나온 순서 그대로
  const groups = [...new Set(SHORTCUT_ACTIONS.map((a) => a.group))].map((name) => ({ name, actions: SHORTCUT_ACTIONS.filter((a) => a.group === name) }));
</script>

<div class="shortcuts" data-testid="shortcuts-group">
  <div class="top">
    <button class="link" disabled={!changedAny} onclick={resetAllShortcuts} data-testid="shortcuts-reset-all">모두 기본값으로</button>
    {#if ShortcutSettings.refused}
      <span class="warn" data-testid="shortcuts-refused">{ShortcutSettings.refused}</span>
    {/if}
    <span class="grow"></span>
    <span class="note">Esc 취소 · Backspace 비우기</span>
  </div>

  {#each groups as group (group.name)}
    <div class="heading">{group.name}</div>
    {#each group.actions as action (action.id)}
      {@const listening = ShortcutSettings.recording === action.id}
      {@const changed = Object.hasOwn(overrides, action.id)}
      {@const current = combo(action.id, overrides)}
      <SettingsCard title={action.title}>
        <div class="control" data-testid={`shortcut-row-${action.id}`}>
          <div class="keys">
            {#if changed}
              <button
                class="reset"
                onclick={() => resetShortcut(action.id)}
                use:hint={`기본값(${action.initial ? label(action.initial) : "없음"})으로 되돌리기`}
                aria-label="기본값으로 되돌리기"
                data-testid={`shortcut-reset-${action.id}`}>↺</button
              >
            {/if}
            <button
              class="key"
              class:listening
              class:changed
              class:empty={!current && !listening}
              onclick={() => (listening ? stopRecording() : startRecording(action.id))}
              onblur={() => ShortcutSettings.recording === action.id && stopRecording()}
              data-testid={`shortcut-key-${action.id}`}
            >
              {listening ? "새 키를 누르세요…" : current ? label(current) : "없음"}
            </button>
          </div>
          {#each conflicts(action.id, overrides) as other (other)}
            <div class="note warn" data-testid={`shortcut-conflict-${action.id}`}>
              "{actionOf(other)?.title ?? other}" 항목과 같은 키입니다
              <button class="link" onclick={() => clearShortcut(other)}>그쪽 비우기</button>
            </div>
          {/each}
          {#if ShortcutSettings.hotKeyFailed.includes(action.id)}
            <div class="note warn" data-testid={`shortcut-hotkey-failed-${action.id}`}>전역 단축키로 등록하지 못했습니다 (다른 앱이 쓰는 키일 수 있습니다)</div>
          {/if}
        </div>
      </SettingsCard>
    {/each}
  {/each}
</div>

<style>
  .shortcuts {
    padding: 14px 18px 24px;
  }
  .top {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-bottom: 8px;
    font-size: 11.5px;
  }
  .grow {
    flex: 1;
  }
  .heading {
    margin: 14px 0 4px;
    font-size: 11.5px;
    font-weight: 600;
    color: var(--fg2);
  }
  .control {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 4px;
  }
  .keys {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .key {
    min-width: 96px;
    height: 26px;
    padding: 0 10px;
    border-radius: 7px;
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 12px;
    font-weight: 500;
    background: rgb(255 255 255 / 0.06);
    box-shadow: inset 0 0 0 1px rgb(255 255 255 / 0.12);
  }
  .key:hover {
    background: rgb(255 255 255 / 0.1);
  }
  .key.changed {
    color: rgb(255 214 10);
  }
  .key.empty {
    color: var(--fg2);
  }
  .key.listening {
    color: rgb(var(--accent));
    background: rgb(var(--accent) / 0.14);
    box-shadow: inset 0 0 0 1px rgb(var(--accent));
  }
  .reset {
    font-size: 13px;
    color: var(--fg2);
  }
  .reset:hover {
    color: var(--fg);
  }
  .note {
    font-size: 11px;
    color: var(--fg2);
  }
  .warn {
    color: rgb(255 159 10);
  }
  .link {
    margin-left: 4px;
    font-size: 11.5px;
    color: rgb(var(--accent));
  }
  .link:disabled {
    color: var(--fg2);
    opacity: 0.5;
  }
  .top .link {
    margin-left: 0;
  }
</style>
