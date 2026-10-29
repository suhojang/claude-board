// 답변 꾸미기(md-render) 명령 규칙(mac mdRenderCommand·mdRenderOff) — 순수 함수다. node:test 로 바로 시험한다.
// 보드 탭에서 치는 claude 명령에서 md-render 의 `--plugin-dir` 을 모두 걷어 내고, 켜져 있고 폴더를 알 때만 하나 붙인다.
// 폴더는 시작 때 백엔드가 `%USERPROFILE%\.claude\notifier\plugins\md-render` 로 옮겨 둔 것이다(src-tauri/src/mdrender.rs).
import { CLAUDE_START } from "./sessionCommand.ts";

// 경로가 `...plugins\md-render` 로 끝나는 `--plugin-dir` 값 하나(따옴표 없음·큰따옴표·작은따옴표). 앞 공백까지 잡아 지운다.
// 사용자의 다른 `--plugin-dir` 은 값이 md-render 로 끝나지 않으므로 남는다. 경로는 대소문자를 가리지 않고 `\`·`/` 어느 쪽이든 읽는다
const MD_RENDER_TAIL = String.raw`[\\/]plugins[\\/]md-render[\\/]?`;
const MD_RENDER_FLAG = new RegExp(
  String.raw`\s+--plugin-dir(?:\s+|=)(?:"[^"]*${MD_RENDER_TAIL}"|'[^']*${MD_RENDER_TAIL}'|[^\s"']*${MD_RENDER_TAIL})(?=\s|$)`,
  "gi",
);
const MD_RENDER_PATH = /[\\/]plugins[\\/]md-render(?:[\\/]|\s|"|'|$)/i;

// 큰따옴표 안에서 cmd.exe·PowerShell 이 풀어 버리는 글자(% $ ` " 굽은 따옴표 ^ !)와 제어 문자·끝의 역슬래시(닫는 따옴표를 가린다)
const NOT_QUOTABLE = /["%$`^!‘’“”„]|\\$|[\u0000-\u001f]/;

/** 폴더를 명령줄에 넣을 꼴(큰따옴표로 묶음)로. 따옴표 안에서도 셸이 풀어 버릴 글자가 있으면 던진다 */
export function quotePluginDir(pluginDir: string): string {
  if (!pluginDir || NOT_QUOTABLE.test(pluginDir)) throw new Error("md-render 폴더 경로를 명령줄에 안전하게 넣을 수 없다");
  return `"${pluginDir}"`;
}

/**
 * 명령의 md-render `--plugin-dir` 을 정리한다 — 끄면(off) 없음, 켜고 pluginDir 을 알면 claude 낱말 바로 뒤에 하나.
 * claude 가 아닌 명령은 그대로. 사용자의 다른 `--plugin-dir` 은 둔다. 큰따옴표 등 다른 꼴로 사용자가 적은 md-render 경로가 남으면 더 붙이지 않는다.
 * pluginDir 에 따옴표 안에서도 풀리는 글자가 있으면 던진다(켜져 있을 때만)
 */
export function mdRenderCommand(command: string, pluginDir: string | null, off: boolean): string {
  const start = CLAUDE_START.exec(command);
  if (!start) return command;
  const stripped = command.replace(MD_RENDER_FLAG, "");
  if (off || pluginDir === null || MD_RENDER_PATH.test(stripped)) return stripped;
  const at = start[0].length;
  return `${stripped.slice(0, at)} --plugin-dir ${quotePluginDir(pluginDir)}${stripped.slice(at)}`;
}

/** 이 세션이 꾸미기를 끈 채 떴는가 */
export function isMdRenderOff(id: string, list: string[]): boolean {
  return list.includes(id);
}

/** 끈 세션 목록에 id 를 더한 새 목록(이미 있으면 같은 내용) */
export function rememberMdRenderOff(list: string[], id: string): string[] {
  return list.includes(id) ? list : [...list, id];
}

/** 아직 있는 세션(liveIds)만 남긴 새 목록 */
export function pruneMdRenderOff(list: string[], liveIds: string[]): string[] {
  const live = new Set(liveIds);
  return list.filter((id) => live.has(id));
}

let pluginDir: string | null = null;

/** 시작 때 받은 옮겨 둔 폴더를 쥔다 — mdRenderLoad.ts 가 부른다 */
export function setPluginDir(dir: string | null) {
  pluginDir = dir;
}

/** 옮겨 둔 md-render 폴더 — 시작 때 한 번 받아 둔다. 모르면 null(플래그를 붙이지 않는다) */
export function currentPluginDir(): string | null {
  return pluginDir;
}
