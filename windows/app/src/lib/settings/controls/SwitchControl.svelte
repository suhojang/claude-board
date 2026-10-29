<script lang="ts">
  // 설정 줄 오른쪽의 켜고 끄는 스위치(mac SettingsSwitch)
  let { on, label, onchange, disabled = false }: { on: boolean; label: string; onchange: (next: boolean) => void; disabled?: boolean } = $props();
</script>

<button type="button" class="switch" class:on role="switch" aria-checked={on} aria-label={label} {disabled} onclick={() => onchange(!on)}>
  <span class="knob"></span>
</button>

<style>
  .switch {
    position: relative;
    flex: none;
    width: 38px;
    height: 22px;
    border-radius: 11px;
    background: rgb(255 255 255 / 0.14);
    box-shadow: inset 0 0 0 0.5px rgb(255 255 255 / 0.18);
    transition: background 0.2s;
  }
  .switch:hover:not(.on):not(:disabled) {
    background: rgb(255 255 255 / 0.22);
  }
  .switch.on {
    background: rgb(var(--accent));
    box-shadow: none;
  }
  .switch:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .knob {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    background: rgb(255 255 255 / 0.8);
    box-shadow: 0 0.5px 1px rgb(0 0 0 / 0.3);
    transition: transform 0.22s cubic-bezier(0.3, 1.3, 0.5, 1), background 0.2s;
  }
  .on .knob {
    transform: translateX(16px);
    background: #fff;
  }
</style>
