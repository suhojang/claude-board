<script lang="ts">
  // 설정 줄 오른쪽의 단추(mac SettingsButton·SettingsLinkButton) — link 는 바탕 없는 글자 단추
  let { text, onclick, prominent = false, link = false, disabled = false }: { text: string; onclick: () => void; prominent?: boolean; link?: boolean; disabled?: boolean } = $props();
</script>

<button type="button" class:prominent class:link {disabled} {onclick}>{text}</button>

<style>
  button {
    padding: 5px 12px;
    border-radius: 7px;
    font-size: 12px;
    color: var(--fg);
    background: rgb(255 255 255 / 0.1);
    white-space: nowrap;
    transition: background 0.15s, opacity 0.15s;
  }
  button:hover:not(:disabled) {
    background: rgb(255 255 255 / 0.17);
  }
  button.prominent {
    background: rgb(var(--accent));
    color: #fff;
    font-weight: 600;
  }
  button.prominent:hover:not(:disabled) {
    background: rgb(var(--accent) / 0.85);
  }
  button.link {
    background: transparent;
    color: var(--fg2);
    padding-inline: 6px;
  }
  button.link:hover:not(:disabled) {
    background: transparent;
    color: var(--fg);
    text-decoration: underline;
  }
  button:disabled {
    opacity: 0.45;
    cursor: default;
  }
</style>
