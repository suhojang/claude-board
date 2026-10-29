// 설정 화면의 상태와 열고 닫기(mac `SettingsScreen`) — 터미널 영역(제목 띠 아래)을 설정 화면으로 덮는다.
// 접힌 보드에서 열면 터미널 칸을 펼치는 것이 전환이 되고 닫을 때 다시 접는다. 열린 동안 탭은 뒤에서 그대로 돈다(터미널 칸은 숨겨지지 않고 가려질 뿐이다).
// 판단은 screenRules.ts(순수, 시험 있음), 여기는 그 결과대로 상태를 바꾸고 백엔드를 부른다.
//
// 한계: 접힌 보드에서 잠깐 펼치는 것도 백엔드는 terminalExpanded 로 저장한다(mac 은 곧바로 false 로 되돌려 쓴다). 닫는 길(닫기·Esc·돌아가기)은
// 모두 다시 접고, 열린 채 보드가 꺼질 때는 끄기 직전(terminal/lifecycle.svelte.ts 의 끌 때 확인 흐름)이 `restoreSettingsBeforeQuit` 를 기다려 접는다.
import { onSettingsEvent, setTerminalExpanded, setTerminalExpandedAndWait, showBoard } from "$lib/backend";
import { board } from "$lib/board/store.svelte";
import { ShortcutSettings, stopRecording } from "$lib/shortcuts/store.svelte";
import type { SettingsSectionId } from "$lib/types";
import { afterClose, collapseOnOpen, escapeStep, shouldDismissForCollapse } from "./screenRules";

export const SettingsScreen = $state({
  /** 설정 화면이 떠 있다 */
  shown: false,
  section: "general" as SettingsSectionId,
  query: "",
  /** 화면에 올라와 있다(내려가는 전환 중에도) — 아니면 내용을 그리지 않는다 */
  visible: false,
  /** 바뀌면 검색 칸에 포커스를 준다 */
  focusSearch: 0,
  /** 내려간 뒤 터미널로 포커스를 돌려줘야 한다(탭을 다루는 동작으로 닫았으면 그쪽이 포커스를 정한다) */
  refocus: false,
});

export interface CloseOptions {
  /** 탭을 다루는 동작으로 닫을 때 — 접힌 보드에서 잠깐 펼쳤어도 펼친 채로 남긴다 */
  keep?: boolean;
}

// 접힌 보드에서 열어 우리가 펼쳤다 — 닫을 때 다시 접는다
let collapsedByUs = false;

const boardState = () => ({ snapKnown: board.snap !== null, expanded: board.snap?.settings.terminalExpanded ?? false });

/** 설정 화면을 연다(section 이 있으면 그 묶음으로). 이미 떠 있으면 묶음만 바꾸고 검색 칸에 포커스를 준다 */
export function openSettings(section?: SettingsSectionId): void {
  if (section) SettingsScreen.section = section;
  showBoard();
  if (SettingsScreen.shown) {
    SettingsScreen.focusSearch += 1;
    return;
  }
  SettingsScreen.query = "";
  SettingsScreen.refocus = false;
  SettingsScreen.shown = true;
  SettingsScreen.visible = true;
  SettingsScreen.focusSearch += 1;
  collapsedByUs = collapseOnOpen(boardState());
  if (collapsedByUs) setTerminalExpanded(true);
}

/** 설정 화면을 닫는다. 떠 있지 않으면 아무것도 하지 않는다 */
export function closeSettings(options: CloseOptions = {}): void {
  if (!SettingsScreen.shown) return;
  const keep = options.keep ?? false;
  SettingsScreen.shown = false;
  SettingsScreen.refocus = !keep;
  if (ShortcutSettings.recording !== null) stopRecording();
  if (afterClose({ collapsedByUs, keep }) === "recollapse") setTerminalExpanded(false);
  collapsedByUs = false;
}

/** 떠 있으면 닫고 아니면 연다 */
export function toggleSettings(): void {
  if (SettingsScreen.shown) closeSettings();
  else openSettings();
}

/** Esc — 키 받는 중이면 취소, 검색어가 있으면 지우고, 아니면 닫는다 */
export function escapeSettings(): void {
  const step = escapeStep({ recording: ShortcutSettings.recording !== null, query: SettingsScreen.query });
  if (step === "cancelRecording") stopRecording();
  else if (step === "clearQuery") SettingsScreen.query = "";
  else closeSettings();
}

/** 왼쪽 목록에서 묶음을 골랐다 — 키 받기를 그만두고 검색어를 비운다 */
export function goSection(section: SettingsSectionId): void {
  if (ShortcutSettings.recording !== null) stopRecording();
  SettingsScreen.section = section;
  SettingsScreen.query = "";
}

/** 터미널 칸이 접혔다(⌃⌥T·접기 단추) — 효과 없이 내린다. 다시 접을 것이 없으니 펼침 기억도 지운다 */
export function dismissForCollapse(): void {
  if (!shouldDismissForCollapse({ shown: SettingsScreen.shown, ...boardState() })) return;
  collapsedByUs = false;
  closeSettings({ keep: true });
  SettingsScreen.visible = false;
}

/** 끄기·업데이트 직전 — 접힌 보드에서 잠깐 펼쳐 둔 것이면 접어 두고 백엔드가 저장할 때까지 기다린다(다시 뜰 때 펼친 채로 뜨지 않게).
 * 접지 못해도 끄기를 막지 않는다 — 실패는 로그만 남긴다 */
export async function restoreSettingsBeforeQuit(): Promise<void> {
  if (!SettingsScreen.shown || !collapsedByUs) return;
  collapsedByUs = false;
  try {
    await setTerminalExpandedAndWait(false);
  } catch (e) {
    console.error("끄기 전에 터미널 칸을 접지 못함", e);
  }
}

/** 설정 화면이 마운트될 때 한 번 — 백엔드의 board://settings 를 듣고, 듣기를 끄는 함수를 돌려준다 */
export function connectSettings(): () => void {
  return onSettingsEvent((payload) => {
    if (payload.open) openSettings(payload.section);
    else closeSettings();
  });
}
