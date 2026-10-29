<script lang="ts">
  // 설정 > 터미널 — 새 세션 기본 명령·답변 꾸미기 기본값(글자·테마·애니메이션은 3단계)
  import { setNewSessionDefaults } from "$lib/backend";
  import { board } from "$lib/board/store.svelte";
  import { DEFAULT_SESSION_COMMAND } from "$lib/terminal/sessionCommand";
  import { currentPluginDir } from "$lib/terminal/mdRender";
  import { defaultNewSessionMdRender, validateDefaultCommand } from "$lib/terminal/newSessionDefaults";
  import SettingsCard from "../SettingsCard.svelte";

  const savedCommand = $derived(board.snap?.settings.newSessionCommand ?? "");
  const mdRender = $derived(defaultNewSessionMdRender(board.snap?.settings));
  // 옮겨 둔 md-render 폴더를 모르면 잠근다(새 세션 창과 같다)
  const mdRenderAvailable = currentPluginDir() !== null;

  // 친 글자 — null 이면 저장된 값을 그대로 보인다
  let draft = $state<string | null>(null);
  const shown = $derived(draft ?? savedCommand);
  const check = $derived(validateDefaultCommand(shown));

  // 올바른 값만 저장한다. 틀리면 사유를 보이고 저장하지 않는다
  function commit() {
    if (!check.ok) return;
    if (check.command !== savedCommand) setNewSessionDefaults(check.command, mdRender);
    draft = null;
  }

  // 칸을 벗어날 때: 올바르면 저장하고, 틀린 글자는 저장하지 않았으니 저장된 값으로 되돌려 보인다
  function leave() {
    commit();
    if (!check.ok) draft = null;
  }

  function keydown(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.isComposing) {
      e.preventDefault();
      commit();
    } else if (e.key === "Escape" && draft !== null) {
      // 고치던 글자를 버리고 칸에서 나온다. 창 전체의 Esc 처리(TerminalAreaView 캡처)는 이 칸의 `data-own-escape` 를 보고 이 Esc 를 가져가지 않는다 —
      // 나온 뒤 한 번 더 누른 Esc 가 설정 화면을 닫는다. 값은 이미 되돌렸으므로 포커스를 풀며 나는 blur 의 저장은 저장된 값 그대로다
      e.preventDefault();
      draft = null;
      (e.currentTarget as HTMLInputElement).blur();
    }
  }

  function toggleMdRender(on: boolean) {
    setNewSessionDefaults(savedCommand, on);
  }
</script>

<div class="group" data-testid="settings-terminal">
  <SettingsCard
    title="기본 명령"
    detail={`새 세션 창과 경로 카드의 + 가 쓰는 명령. 비우면 ${DEFAULT_SESSION_COMMAND}`}
  >
    <div class="field-box">
      <input
        class="field"
        class:bad={!check.ok}
        type="text"
        spellcheck="false"
        autocomplete="off"
        placeholder={DEFAULT_SESSION_COMMAND}
        value={shown}
        oninput={(e) => (draft = e.currentTarget.value)}
        onblur={leave}
        onkeydown={keydown}
        data-own-escape={draft !== null ? "" : undefined}
        data-testid="settings-session-command"
      />
      {#if !check.ok}<div class="error" data-testid="settings-session-command-error">{check.reason}</div>{/if}
    </div>
  </SettingsCard>

  <SettingsCard
    title="답변 마크다운 꾸미기"
    detail="새 세션을 열 때 기본으로 켭니다(md-render). 새 세션 창에서 세션마다 바꿀 수 있습니다"
  >
    <input
      type="checkbox"
      checked={mdRender}
      disabled={!mdRenderAvailable}
      onchange={(e) => toggleMdRender(e.currentTarget.checked)}
      data-testid="settings-session-md"
    />
  </SettingsCard>
</div>

<style>
  .group {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .field-box {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 4px;
  }
  .field {
    box-sizing: border-box;
    width: 280px;
    height: 22px;
    padding: 0 6px;
    border-radius: 5px;
    border: 0.5px solid rgb(255 255 255 / 0.2);
    background: rgb(255 255 255 / 0.07);
    color: var(--fg);
    font-family: var(--mono);
    font-size: 13px;
    outline: none;
  }
  .field:focus {
    box-shadow: 0 0 0 3px rgb(var(--working) / 0.45);
  }
  .field.bad {
    border-color: rgb(255 99 99 / 0.8);
  }
  .error {
    font-size: 11px;
    color: rgb(255 120 120);
  }
</style>
