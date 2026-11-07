<script lang="ts">
  // 자율주행 시작 준비 화면 — 프로젝트 폴더, 단위(제목·목표·작업 폴더·판정 명령·상한), 실행 환경, 모델을 정하고 출발한다.
  // 칸 검사와 출발 요청은 startForm.ts, 출발은 백엔드가 `claude-autopilot start` 를 불러 한다. 거절하면 그 글이 아래에 그대로 뜬다.
  // 창 맨 위 가운데에서 내려오는 모달이다(새 세션 창과 같은 모양). Esc 는 닫기(쓴 칸은 남는다), Ctrl+Enter 는 시작
  import { pickFolder } from "./pickFolder";
  import { AutopilotStart as S, addUnit, chooseEnv, closeAutopilotStart, removeUnit, submitStart } from "./start.svelte";
  import { MAX_UNITS, MODEL_SUGGESTIONS, envOf, needsConsent, problemFor, syntaxHint, validateDraft } from "./startForm";

  const problems = $derived(validateDraft(S.draft, S.envs));
  const env = $derived(envOf(S.draft, S.envs));
  const consentNeeded = $derived(needsConsent(env));
  // 시작을 눌러 본 뒤에만 칸마다 문제 글을 보인다
  const say = (field: string) => (S.tried ? problemFor(problems, field) : null);

  async function pickProject() {
    const picked = await pickFolder(S.draft.projectDir);
    if (picked !== null) S.draft.projectDir = picked;
  }

  async function pickUnitDir(index: number) {
    const unit = S.draft.units[index];
    const picked = await pickFolder(unit.cwd || S.draft.projectDir);
    if (picked !== null) unit.cwd = picked;
  }

  // 보드 쪽 칸 위까지 창 전체를 덮도록 body 로 옮긴다
  function toBody(node: HTMLElement) {
    document.body.appendChild(node);
    return { destroy: () => node.remove() };
  }

  // xterm 이 Esc 를 먼저 먹으므로 캡처로 받는다
  function keydown(e: KeyboardEvent) {
    if (e.key === "Escape" && !e.isComposing) {
      e.preventDefault();
      e.stopPropagation();
      closeAutopilotStart();
    } else if (e.key === "Enter" && e.ctrlKey && !e.isComposing) {
      e.preventDefault();
      e.stopPropagation();
      void submitStart();
    }
  }

  function focusFirst(node: HTMLInputElement) {
    const frame = requestAnimationFrame(() => node.focus());
    return { destroy: () => cancelAnimationFrame(frame) };
  }
</script>

<svelte:window onkeydowncapture={keydown} />

<div class="scrim" role="presentation" use:toBody>
  <div class="sheet" role="dialog" aria-modal="true" aria-label="자율주행 시작 준비" data-testid="autopilot-start">
    <div class="head">
      <div class="title">자율주행 시작 준비</div>
      <button class="x" onclick={closeAutopilotStart} aria-label="닫기">×</button>
    </div>

    <div class="body">
      <section class="group">
        <div class="label">프로젝트 폴더</div>
        <div class="row">
          <input class="field grow" type="text" spellcheck="false" autocomplete="off" placeholder="C:\work\app" bind:value={S.draft.projectDir} use:focusFirst aria-label="프로젝트 폴더" />
          <button class="push" onclick={pickProject}>고르기…</button>
        </div>
        {#if say("projectDir")}<div class="err">{say("projectDir")}</div>{/if}
        <input class="field" type="text" spellcheck="false" autocomplete="off" placeholder="실행 이름 (비우면 폴더 이름)" bind:value={S.draft.title} aria-label="실행 이름" />
        {#if say("title")}<div class="err">{say("title")}</div>{/if}
      </section>

      <section class="group">
        <div class="label">단위 {S.draft.units.length}개</div>
        <div class="caption">위에서 아래로 차례로 돕니다. 단위마다 판정 명령이 통과해야 끝납니다.</div>
        {#each S.draft.units as u, i (u)}
          <div class="unit" data-testid="autopilot-unit">
            <div class="unitHead">
              <span class="num">단위 {i + 1}</span>
              {#if S.draft.units.length > 1}
                <button class="link" onclick={() => removeUnit(i)}>빼기</button>
              {/if}
            </div>
            <input class="field" type="text" placeholder="제목" bind:value={u.title} aria-label={`단위 ${i + 1} 제목`} />
            {#if say(`unit.${i + 1}.title`)}<div class="err">{say(`unit.${i + 1}.title`)}</div>{/if}
            <textarea class="field area" rows="3" placeholder="목표 (모델에게 처음 보낼 글)" bind:value={u.goal} aria-label={`단위 ${i + 1} 목표`}></textarea>
            {#if say(`unit.${i + 1}.goal`)}<div class="err">{say(`unit.${i + 1}.goal`)}</div>{/if}
            <div class="row">
              <input class="field grow" type="text" spellcheck="false" autocomplete="off" placeholder="작업 폴더 (비우면 프로젝트 폴더)" bind:value={u.cwd} aria-label={`단위 ${i + 1} 작업 폴더`} />
              <button class="push" onclick={() => pickUnitDir(i)}>고르기…</button>
            </div>
            <input class="field mono" type="text" spellcheck="false" autocomplete="off" placeholder="판정 명령 (예: npm test)" bind:value={u.check} aria-label={`단위 ${i + 1} 판정 명령`} />
            {#if say(`unit.${i + 1}.check`)}<div class="err">{say(`unit.${i + 1}.check`)}</div>{/if}
            <div class="caps">
              <label class="cap"><span>시간 상한(분)</span><input class="field narrow" type="text" inputmode="decimal" placeholder="기본값" bind:value={u.minutes} /></label>
              <label class="cap"><span>비용 상한(USD)</span><input class="field narrow" type="text" inputmode="decimal" placeholder="기본값" bind:value={u.usd} /></label>
            </div>
            {#if say(`unit.${i + 1}.minutes`)}<div class="err">{say(`unit.${i + 1}.minutes`)}</div>{/if}
            {#if say(`unit.${i + 1}.usd`)}<div class="err">{say(`unit.${i + 1}.usd`)}</div>{/if}
          </div>
        {/each}
        <button class="push wide" onclick={addUnit} disabled={S.draft.units.length >= MAX_UNITS}>+ 단위 추가</button>
        {#if say("units")}<div class="err">{say("units")}</div>{/if}
        <div class="caption">{syntaxHint(env)} 상한을 비우면 엔진의 기본 상한을 씁니다.</div>
      </section>

      <section class="group">
        <div class="label">실행 환경</div>
        <div class="caption">모델이 돌 곳입니다. 기본값이 없으니 직접 고르세요.</div>
        {#if S.envsLoaded && S.envs.length === 0}
          <div class="err">이 PC 에서 고를 수 있는 실행 환경을 찾지 못했습니다.</div>
        {/if}
        <div class="envs" role="radiogroup" aria-label="실행 환경">
          {#each S.envs as e (e.key)}
            {@const on = S.draft.envKey === e.key}
            <button class="envRow" class:on role="radio" aria-checked={on} onclick={() => chooseEnv(e.key)}>
              <svg class="radio" class:on viewBox="0 0 16 16" width="15" height="15" aria-hidden="true">
                <circle cx="8" cy="8" r="6.5" fill="none" stroke="currentColor" stroke-width="1.3" />
                {#if on}<circle cx="8" cy="8" r="3.6" fill="currentColor" />{/if}
              </svg>
              <span class="envName">{e.label}</span>
              <span class="chip" class:safe={e.sandboxCapable}>{e.sandboxCapable ? "샌드박스 가능" : "샌드박스 없음"}</span>
            </button>
          {/each}
        </div>
        {#if say("env")}<div class="err">{say("env")}</div>{/if}
        {#if consentNeeded}
          <label class="consent" class:bad={say("consent")}>
            <input type="checkbox" bind:checked={S.draft.consent} />
            <span>
              <strong>샌드박스 없이 돌리는 데 동의합니다.</strong>
              이 환경에서는 Claude Code 샌드박스가 걸리지 않고 도구 권한 규칙만 벽입니다. 규칙이 막지 못하는 일이 있습니다.
            </span>
          </label>
          {#if say("consent")}<div class="err">{say("consent")}</div>{/if}
        {/if}
      </section>

      <section class="group">
        <div class="label">모델</div>
        <input class="field mono" type="text" spellcheck="false" autocomplete="off" list="autopilot-models" bind:value={S.draft.model} aria-label="모델" />
        <datalist id="autopilot-models">
          {#each MODEL_SUGGESTIONS as m (m)}<option value={m}></option>{/each}
        </datalist>
        {#if say("model")}<div class="err">{say("model")}</div>{/if}
      </section>
    </div>

    <div class="foot">
      {#if S.error}
        <pre class="fail" data-testid="autopilot-start-error" role="alert">{S.error}</pre>
      {:else if S.tried && problems.length > 0}
        <div class="fail">빠진 칸이 {problems.length}곳 있습니다. 붉은 글을 확인하세요.</div>
      {/if}
      <div class="buttons">
        <span class="hintKey">Ctrl+Enter 로 시작</span>
        <button class="push" onclick={closeAutopilotStart}>닫기</button>
        <button class="push default" onclick={() => void submitStart()} disabled={S.starting} data-testid="autopilot-start-go">{S.starting ? "출발하는 중…" : "시작"}</button>
      </div>
    </div>
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 40;
    display: flex;
    justify-content: center;
    align-items: flex-start;
    overflow: hidden;
    background: rgb(0 0 0 / 0.35);
    animation: scrim-in 0.2s ease-out;
  }
  .sheet {
    box-sizing: border-box;
    width: 620px;
    max-width: 100%;
    max-height: 100%;
    display: flex;
    flex-direction: column;
    border-radius: 0 0 12px 12px;
    background: rgb(40 40 58 / 0.98);
    font-size: 13px;
    box-shadow:
      inset 0 0 0 0.5px rgb(255 255 255 / 0.16),
      0 12px 32px rgb(0 0 0 / 0.55);
    animation: sheet-in 0.25s ease-out;
  }
  @keyframes sheet-in {
    from {
      transform: translateY(-100%);
    }
  }
  @keyframes scrim-in {
    from {
      opacity: 0;
    }
  }
  .head {
    flex: none;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 14px 18px 8px;
  }
  .title {
    font-size: 16px;
    font-weight: 700;
  }
  .x {
    width: 22px;
    height: 22px;
    border-radius: 11px;
    font-size: 16px;
    line-height: 20px;
    color: var(--fg2);
  }
  .x:hover {
    color: var(--fg);
    background: rgb(255 255 255 / 0.1);
  }
  .body {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    padding: 4px 18px 12px;
    display: flex;
    flex-direction: column;
    gap: 16px;
  }
  .group {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .label {
    font-size: 12px;
    font-weight: 600;
    color: var(--fg2);
  }
  .caption {
    font-size: 11px;
    color: var(--fg2);
  }
  .err {
    font-size: 11px;
    color: rgb(var(--red));
  }
  .row {
    display: flex;
    gap: 6px;
  }
  .grow {
    flex: 1;
    min-width: 0;
  }
  .field {
    box-sizing: border-box;
    min-height: 22px;
    padding: 2px 6px;
    border-radius: 5px;
    border: 0.5px solid rgb(255 255 255 / 0.2);
    background: rgb(255 255 255 / 0.07);
    color: var(--fg);
    font-size: 13px;
    outline: none;
  }
  .field.mono {
    font-family: var(--mono);
  }
  .field.area {
    resize: vertical;
    line-height: 1.4;
    font-family: inherit;
  }
  .field.narrow {
    width: 84px;
  }
  .field:focus {
    box-shadow: 0 0 0 3px rgb(var(--working) / 0.45);
  }
  .unit {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 10px;
    border-radius: 10px;
    background: rgb(255 255 255 / 0.05);
  }
  .unitHead {
    display: flex;
    justify-content: space-between;
    align-items: center;
    font-size: 12px;
    font-weight: 600;
  }
  .link {
    font-size: 11px;
    color: rgb(var(--accent));
  }
  .caps {
    display: flex;
    flex-wrap: wrap;
    gap: 6px 14px;
  }
  .cap {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 11px;
    color: var(--fg2);
  }
  .envs {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .envRow {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 10px;
    border-radius: 8px;
    text-align: left;
    color: var(--fg);
  }
  .envRow.on {
    background: rgb(var(--working) / 0.16);
  }
  .radio {
    flex: none;
    color: var(--fg2);
  }
  .radio.on {
    color: rgb(var(--working));
  }
  .envName {
    flex: 1;
    min-width: 0;
    font-weight: 600;
  }
  .chip {
    flex: none;
    padding: 1.5px 7px;
    border-radius: 999px;
    font-size: 10px;
    font-weight: 600;
    color: rgb(var(--orange));
    background: rgb(var(--orange) / 0.14);
  }
  .chip.safe {
    color: rgb(var(--green));
    background: rgb(var(--green) / 0.14);
  }
  .consent {
    display: flex;
    gap: 8px;
    align-items: flex-start;
    padding: 8px 10px;
    border-radius: 8px;
    font-size: 12px;
    line-height: 1.45;
    color: var(--fg2);
    background: rgb(var(--orange) / 0.1);
  }
  .consent.bad {
    box-shadow: inset 0 0 0 1px rgb(var(--red) / 0.7);
  }
  .consent strong {
    display: block;
    color: var(--fg);
  }
  .consent input {
    margin: 2px 0 0;
    flex: none;
  }
  .foot {
    flex: none;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px 18px 14px;
    border-top: 0.5px solid rgb(255 255 255 / 0.1);
  }
  .fail {
    margin: 0;
    max-height: 120px;
    overflow: auto;
    padding: 8px 10px;
    border-radius: 8px;
    font: inherit;
    font-size: 12px;
    line-height: 1.45;
    white-space: pre-wrap;
    word-break: break-word;
    color: rgb(var(--red));
    background: rgb(var(--red) / 0.12);
  }
  .buttons {
    display: flex;
    justify-content: flex-end;
    align-items: center;
    gap: 8px;
  }
  .hintKey {
    margin-right: auto;
    font-size: 11px;
    color: var(--fg3);
  }
  .push {
    height: 22px;
    padding: 0 12px;
    border-radius: 5px;
    font-size: 13px;
    background: rgb(255 255 255 / 0.12);
    box-shadow: inset 0 0 0 0.5px rgb(255 255 255 / 0.16);
    color: var(--fg);
    white-space: nowrap;
  }
  .push.wide {
    align-self: flex-start;
  }
  .push:hover:not(:disabled) {
    background: rgb(255 255 255 / 0.18);
  }
  .push.default {
    background: #0a84ff;
    box-shadow: none;
    color: #fff;
  }
  .push.default:hover:not(:disabled) {
    background: #3d9bff;
  }
  .push:disabled {
    opacity: 0.45;
  }
</style>
