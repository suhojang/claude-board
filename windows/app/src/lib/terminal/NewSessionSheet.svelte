<script lang="ts">
  // 새 세션 창(ClaudeBoard.swift NewSessionSheet) — 작업 폴더(보드에 있는 경로 + 다른 폴더)와 실행할 명령을 고르고 새 탭에서 시작한다.
  // 창 맨 위 가운데에서 내려오는 모달이다(mac 의 시트). Esc 는 취소, Enter 는 시작
  import { inTauri } from "$lib/backend";
  import { board } from "$lib/board/store.svelte";
  import { mockPickFolder } from "$lib/mock";
  import { focusActive, TerminalHub, workspaceHint, workspaceKey } from "./hub.svelte";
  import { DEFAULT_SESSION_COMMAND, startSession } from "./launch";
  import { currentPluginDir } from "./mdRender";

  // 보드에 있는 경로(전체 경로). 열 때 한 번 정한다
  const paths = (board.snap?.groups ?? []).map((group) => group.id);
  // 지금 경로와 같은 보드 경로(없으면 첫 경로, 보드에 경로가 없으면 홈)
  const initial = paths.find((path) => workspaceKey(path) === workspaceKey(TerminalHub.workspace)) ?? paths[0] ?? "~";

  let dir = $state(initial);
  let others = $state<string[]>(paths.includes(initial) ? [] : [initial]);
  let command = $state(DEFAULT_SESSION_COMMAND);
  // 답변 마크다운 꾸미기 — 기본 켬. 옮겨 둔 md-render 폴더를 모르면 잠근다(mac 도 폴더가 없으면 잠근다)
  let mdRender = $state(true);
  const mdRenderAvailable = currentPluginDir() !== null;

  const rows = $derived([...paths, ...others]);
  const canStart = $derived(command.trim() !== "");

  const lastComponent = (path: string) => path.replace(/[\\/]+$/, "").split(/[\\/]/).at(-1) || path;

  function cancel() {
    TerminalHub.sheet = false;
    focusActive(); // 취소해도 키 입력이 터미널로 돌아오게
  }

  function start() {
    if (!canStart) return;
    TerminalHub.sheet = false;
    startSession(command, dir, mdRender);
  }

  async function pick() {
    let picked: string | string[] | null;
    if (inTauri) {
      const { open } = await import("@tauri-apps/plugin-dialog");
      picked = await open({ directory: true, multiple: false, defaultPath: dir === "~" ? undefined : dir });
    } else {
      picked = mockPickFolder();
    }
    // 고르든 취소하든 포커스는 명령 칸으로 — 안 그러면 단추나 창 뒤 터미널에 남아 쳐 넣은 글자가 엉뚱한 데로 간다
    focusCommand();
    if (typeof picked !== "string") return;
    // 같은 폴더를 다른 꼴(슬래시 방향·대소문자·끝 \)로 고르면 새 줄을 만들지 않고 있던 줄을 고른다
    const same = rows.find((path) => workspaceKey(path) === workspaceKey(picked));
    if (same) dir = same;
    else {
      others.push(picked);
      dir = picked;
    }
  }

  // 보드 쪽 칸 위까지 창 전체를 덮도록 body 로 옮긴다(터미널 칸 안에 두면 오른쪽 보드가 위에 그려진다)
  function toBody(node: HTMLElement) {
    document.body.appendChild(node);
    return { destroy: () => node.remove() };
  }

  // 열리면 명령 칸이 포커스를 쥔다 — 안 그러면 xterm 의 textarea 가 포커스를 쥔 채 쳐 넣은 글자가 창 뒤 셸로 들어간다.
  // toBody 가 창을 body 로 옮기면 포커스가 풀리므로 다음 프레임에 준다
  let commandField = $state<HTMLInputElement>();
  function focusCommand() {
    commandField?.focus();
    commandField?.select();
  }

  function focusSelect(node: HTMLInputElement) {
    const frame = requestAnimationFrame(() => {
      node.focus();
      node.select();
    });
    return { destroy: () => cancelAnimationFrame(frame) };
  }

  // xterm 이 Esc·Enter 를 먼저 먹으므로 캡처로 받는다. 단추에 포커스가 있을 때의 Enter 는 그 단추의 누름에 맡긴다
  function keydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      cancel();
    } else if (e.key === "Enter" && !e.isComposing && !(e.target instanceof HTMLButtonElement)) {
      e.preventDefault();
      e.stopPropagation();
      start();
    }
  }
</script>

<svelte:window onkeydowncapture={keydown} />

<div class="scrim" role="presentation" use:toBody>
  <div class="sheet" role="dialog" aria-modal="true" aria-label="새 세션">
    <div class="title">새 세션</div>

    <div class="group">
      <div class="label">작업 폴더 (보드에 있는 경로)</div>
      <div class="list" style:height="{Math.min(rows.length * 46, 230)}px">
        {#each rows as path (path)}
          {@const on = path === dir}
          <button class="row" class:on onclick={() => (dir = path)}>
            <svg class="radio" class:on viewBox="0 0 16 16" width="15" height="15" aria-hidden="true">
              <circle cx="8" cy="8" r="6.5" fill="none" stroke="currentColor" stroke-width="1.3" />
              {#if on}<circle cx="8" cy="8" r="3.6" fill="currentColor" />{/if}
            </svg>
            <span class="texts">
              <span class="name">{lastComponent(path)}</span>
              <span class="path"><bdi>{workspaceHint(path)}</bdi></span>
            </span>
          </button>
        {/each}
      </div>
      <button class="push" onclick={pick}>다른 폴더 고르기…</button>
    </div>

    <div class="group">
      <div class="label">실행할 명령</div>
      <input class="field" type="text" spellcheck="false" autocomplete="off" bind:value={command} bind:this={commandField} use:focusSelect />
      <div class="caption">작업 경로의 셸에 쳐 넣어 실행합니다. claude 면 보드가 --session-id 를 붙입니다</div>
    </div>

    <div class="group tight">
      <label class="check" class:disabled={!mdRenderAvailable}>
        <input type="checkbox" bind:checked={mdRender} disabled={!mdRenderAvailable} />
        <span>답변 마크다운 꾸미기</span>
      </label>
      <div class="caption">답변이 끝나면 제목·굵게·인라인 코드·목록 등을 색으로 다시 그립니다(md-render). claude 면 --plugin-dir 로 붙입니다</div>
    </div>

    <div class="buttons">
      <button class="push" onclick={cancel}>취소</button>
      <button class="push default" onclick={start} disabled={!canStart}>새 탭에서 시작</button>
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
  /* 맨 위 가운데에서 내려온다 */
  .sheet {
    box-sizing: border-box;
    width: 460px;
    max-width: 100%;
    padding: 18px;
    display: flex;
    flex-direction: column;
    gap: 14px;
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
  .title {
    font-size: 16px;
    font-weight: 700;
  }
  .group {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 6px;
  }
  .group.tight {
    gap: 4px;
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
  .list {
    align-self: stretch;
    display: flex;
    flex-direction: column;
    gap: 2px;
    overflow-y: auto;
    scrollbar-width: none;
  }
  .list::-webkit-scrollbar {
    display: none;
  }
  .row {
    flex: none;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 10px;
    border-radius: 8px;
    text-align: left;
    color: var(--fg);
  }
  .row.on {
    background: rgb(var(--working) / 0.16);
  }
  .radio {
    flex: none;
    color: var(--fg2);
  }
  .radio.on {
    color: rgb(var(--working));
  }
  .texts {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .name {
    font-size: 13px;
    font-weight: 600;
  }
  .path {
    font-size: 11px;
    color: var(--fg2);
    white-space: nowrap;
    overflow: hidden;
  }
  .field {
    align-self: stretch;
    box-sizing: border-box;
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
  .check {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 13px;
  }
  .check.disabled {
    color: var(--fg3);
  }
  .check input {
    margin: 0;
  }
  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
  .push {
    height: 22px;
    padding: 0 12px;
    border-radius: 5px;
    font-size: 13px;
    background: rgb(255 255 255 / 0.12);
    box-shadow: inset 0 0 0 0.5px rgb(255 255 255 / 0.16);
    color: var(--fg);
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
