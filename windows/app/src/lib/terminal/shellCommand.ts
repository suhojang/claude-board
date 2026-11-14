// 셸 명령 글자를 낱말 단위로 읽고 고치는 규칙 — mac ShellCommand.swift 를 Windows 판(bash → cmd.exe·PowerShell)으로 옮긴 것이다.
// 보드가 탭 셸에 치는 claude 명령(보드가 만든 명령·사용자가 친 명령)에 답변 꾸미기(--plugin-dir …\plugins\md-render)를 빼고 붙이고,
// 세션 이름 --name 을 끼우고, --session-id 를 바꾼다. 3.3.2 까지는 따옴표로 묶인 글자 위에서 정규식으로 지우고 끼워, 첫 요청 글 같은
// 묶인 낱말 안을 지우거나 따옴표 경계를 지워 셸이 다르게 읽을 수 있었다(보안 검토 [높음] — 되살리기의 --session-id 지우기가 첫 요청
// 글 끝의 닫는 ' 를 함께 지워 다음 인자가 명령이 됨). 그래서 낱말을 원문 자리와 함께 읽고, 낱말 범위만 지우거나 그 사이에 끼우며
// (남긴 낱말은 원문 글자 그대로라 따옴표 꼴이 바뀌지 않는다), 고친 글자를 다시 읽어 뜻한 낱말과 같은지 확인한다.
// 자리는 글자가 아니라 유니코드 스칼라로 잡는다 — ' 바로 뒤에 결합 문자가 오면 첫 글자(첫 단위) 비교로는 ' 를 바꾸지 못한다(3.3.2 의 함정).
// Windows 어댑터차: cmd.exe·PowerShell 은 작은따옴표를 인용으로 읽지 않는다. 그래서 shellEdit 이 끼우는 값(afterFirst·append)은
// shlex 꼴로 묶지 않고 그대로 넣는다 — 부르는 쪽(sessionNameArg·quotePluginDir)이 큰따옴표 꼴을 주고, 값은 "낱말 하나" 로 다시 읽혀 검사한다.

// 낱말 판정에 쓰는 안전한 글자 — 여기만 든 인자(맨 앞 = 는 제외)는 따옴표 없이도 셸이 글자 그대로 넘긴다
const SHELL_SAFE_CHARS = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_@%+=:,./-";
// shlex.quote 의 안전 판정과 같다 — 이 밖의 글자가 하나라도 있으면 작은따옴표로 묶는다
const SHLEX_UNSAFE = /[^A-Za-z0-9_@%+=:,./-]/;
// shlex.quote 꼴 — 작은따옴표를 끝내고 큰따옴표로 넣고 다시 시작한다
const SHLEX_QUOTED_QUOTE = "'\"'\"'";

/** shlex.quote 와 같은 인용 — 안전한 글자만 든 것은 그대로 둔다. ' 는 글자 그대로 바꾼다(' 뒤 결합 문자는 한 글자로 묶여 보이지 않는다) */
export function shlexQuote(s: string): string {
  if (s.length > 0 && !SHLEX_UNSAFE.test(s)) return s;
  return "'" + s.replace(/'/g, () => SHLEX_QUOTED_QUOTE) + "'";
}

/** 보드가 셸에 새로 치는 인자 하나(shlexQuote 꼴) — 따옴표 없이 = 로 시작하면 묶는다(zsh 는 =이름 을 그 명령의 경로로 펼친다) */
export function shellArg(s: string): string {
  const quoted = shlexQuote(s);
  return quoted.startsWith("=") ? "'" + quoted + "'" : quoted;
}

/** 글자(UTF-16 단위) 자리와 유니코드 스칼라 경계를 맞춘다 — at: 단위→스칼라(짝의 낮은 반쪽도 같은 스칼라), from: 스칼라→단위(끝 자리 n 포함) */
function scalarIndexMap(s: string): { at: number[]; from: number[] } {
  const n = s.length;
  const at: number[] = new Array(n + 1).fill(-1);
  const from: number[] = [];
  let scalar = 0;
  let i = 0;
  while (i < n) {
    const high = s.charCodeAt(i) >= 0xd800 && s.charCodeAt(i) <= 0xdbff;
    const low = high && i + 1 < n && s.charCodeAt(i + 1) >= 0xdc00 && s.charCodeAt(i + 1) <= 0xdfff;
    at[i] = scalar;
    from.push(i);
    if (low) {
      at[i + 1] = scalar;
      scalar++;
      i += 2;
    } else {
      scalar++;
      i++;
    }
  }
  from.push(n);
  at[n] = scalar;
  return { at, from };
}

/** 셸 낱말 하나 — 푼 값(text)과 원문 자리(유니코드 스칼라 경계, start…end), 셸이 글자 그대로 넘기는지(literal). 상태는 바꾸지 않는다 */
export class ShellWord {
  readonly text: string;
  readonly start: number;
  readonly end: number;
  readonly literal: boolean;

  constructor(text: string, start: number, end: number, literal: boolean) {
    this.text = text;
    this.start = start;
    this.end = end;
    this.literal = literal;
  }

  /** 원문에서 이 낱말의 글자 — 시작 ' 와 이어 쓰기의 \·줄바꿈, 이스케이프 글자까지 그대로 든다 */
  raw(s: string): string {
    const { from } = scalarIndexMap(s);
    return s.slice(from[this.start], from[this.end]);
  }
}

/** 셸 명령 한 줄을 읽은 것 */
export interface ShellLine {
  words: ShellWord[];
  /** 셸이 이 낱말들을 한 명령의 인자로 글자 그대로 넘기는가 — 모든 낱말이 literal 이고 낱말 사이가 공백·탭뿐(따옴표 밖 줄바꿈 없음) */
  plain: boolean;
  texts: string[];
}

/**
 * 셸 명령 글자를 낱말로 읽는다 — 보드가 남긴 명령(shlex.quote 꼴)과 사람이 적은 명령의 흔한 꼴: 공백·탭·줄바꿈으로 나누고 '…'(그대로)·
 * "…(\ 는 " \ $ ` 줄바꿈 앞에서만 풀린다)·따옴표 밖 \ 를 푼다(\줄바꿈 은 이어 쓰기라 둘 다 없어진다). 변수·명령 치환·연산자(; | & 등)는
 * 풀지 않고 글자 그대로 낱말에 두며 그 낱말은 literal 이 아니다. 따옴표가 닫히지 않았거나 끝이 \ 이면 없다(뒤에 붙일 글자를 삼키므로).
 * 자리는 유니코드 스칼라 경계다 — 낱말 사이 공백·줄바꿈은 스칼라 하나다. */
export function shellLine(s: string): ShellLine | null {
  const n = s.length;
  const { at } = scalarIndexMap(s);
  const words: ShellWord[] = [];
  let plain = true;
  let cur = "";
  let start = -1;
  let literal = true;
  let i = 0;
  while (i < n) {
    const code = s.charCodeAt(i);
    if (code === 0x20 || code === 0x09 || code === 0x0a) {
      if (start >= 0) {
        words.push(new ShellWord(cur, start, at[i], literal));
        cur = "";
        start = -1;
        literal = true;
      }
      if (code === 0x0a) plain = false;
      i++;
      continue;
    }
    if (code === 0x27) {
      // 작은따옴표 — 그대로
      if (start < 0) start = at[i];
      let j = i + 1;
      while (j < n && s.charCodeAt(j) !== 0x27) {
        cur += s.charAt(j);
        j++;
      }
      if (j >= n) return null;
      i = j + 1;
      continue;
    }
    if (code === 0x22) {
      // 큰따옴표 — \ 는 " \ $ ` 줄바꿈 앞에서만 풀린다. $ ` ! 는 큰따옴표 안에서도 셸이 풀어 버리므로 literal 이 아니다
      if (start < 0) start = at[i];
      let j = i + 1;
      for (;;) {
        if (j >= n) return null;
        const d = s.charCodeAt(j);
        if (d === 0x22) break;
        if (d === 0x5c) {
          const next = j + 1;
          const escaped = next < n ? s.charCodeAt(next) : -1;
          if (escaped === 0x22 || escaped === 0x5c || escaped === 0x24 || escaped === 0x60 || escaped === 0x0a) {
            if (escaped !== 0x0a) cur += s.charAt(next);
            j = next + 1;
            continue;
          }
        }
        if (d === 0x24 || d === 0x60 || d === 0x21) literal = false;
        cur += s.charAt(j);
        j++;
      }
      i = j + 1;
      continue;
    }
    if (code === 0x5c) {
      // 따옴표 밖 \ — 다음 단위를 그대로 넣는다(짝이면 두 단위를 하나로). \줄바꿈 은 이어 쓰기라 없어진다
      const next = i + 1;
      if (next >= n) return null;
      if (s.charCodeAt(next) === 0x0a) {
        i = next + 1;
        continue;
      }
      if (start < 0) start = at[i];
      const escaped = s.charCodeAt(next);
      const pair =
        escaped >= 0xd800 && escaped <= 0xdbff && next + 1 < n && s.charCodeAt(next + 1) >= 0xdc00 && s.charCodeAt(next + 1) <= 0xdfff;
      cur += pair ? s.slice(next, next + 2) : s.charAt(next);
      i = pair ? next + 2 : next + 1;
      continue;
    }
    if (start < 0) {
      start = at[i];
      if (code === 0x3d) literal = false; // 맨 앞 = (zsh 의 =이름 펼침)
    }
    if (!SHELL_SAFE_CHARS.includes(String.fromCharCode(code))) literal = false;
    const pair = code >= 0xd800 && code <= 0xdbff && i + 1 < n && s.charCodeAt(i + 1) >= 0xdc00 && s.charCodeAt(i + 1) <= 0xdfff;
    cur += pair ? s.slice(i, i + 2) : s.charAt(i);
    i += pair ? 2 : 1;
  }
  if (start >= 0) words.push(new ShellWord(cur, start, at[n], literal));
  return { words, plain: plain && words.every((word) => word.literal), texts: words.map((word) => word.text) };
}

/** 셸 명령 글자를 낱말 값으로(shellLine 의 값만). 읽지 못하면 없다 */
export function shellWords(s: string): string[] | null {
  const line = shellLine(s);
  return line === null ? null : line.texts;
}

/** 인자 목록을 셸 명령 글자로 — 인자마다 shellArg 로 묶어 공백으로 잇고, 다시 읽어 셸이 그 인자 그대로 넘기는지(plain) 확인한다. 아니면 없다 */
export function shellCommand(argv: readonly string[]): string | null {
  const parts: string[] = [];
  for (const arg of argv) parts.push(shellArg(arg));
  const command = parts.join(" ");
  const line = shellLine(command);
  if (line === null || !line.plain || line.texts.length !== argv.length) return null;
  for (let i = 0; i < argv.length; i++) {
    if (line.texts[i] !== argv[i]) return null;
  }
  return command;
}

/** 끼우는 값 하나가 셸에서 낱말 하나로 읽히는지 보고 그 값(푼 글자)을 돌려준다 — 아니면 없다(개행은 낱말을 나누므로) */
export function singleWordText(value: string): string | null {
  const line = shellLine(value);
  if (line === null || line.words.length !== 1) return null;
  return line.texts[0];
}

/** 낱말 단위 고치기 — remove 의 낱말(차례 번호)을 앞 공백과 함께 지우고(이어진 차례는 한 덩어리로, oneSpace 면 덩어리 앞 공백은 한 글자만),
 * afterFirst 를 첫 낱말 바로 뒤에, append 를 끝에 끼운다. 끼우는 값은 꼴(원문)을 그대로 넣고, 값은 낱말 하나로 다시 읽어 검사한다
 * (Windows 어댑터차 — cmd·PowerShell 은 작은따옴표를 인용으로 읽지 않는다). 남긴 낱말과 그 사이 공백은 원문 글자 그대로다.
 * 고친 글자를 다시 읽어 (남긴 낱말 + 끼운 값)과 같지 않으면 없다 */
export type ShellEditOptions = {
  remove?: readonly number[];
  oneSpace?: boolean;
  afterFirst?: readonly string[];
  append?: readonly string[];
};

export function shellEdit(command: string, line: ShellLine, edit: ShellEditOptions): string | null {
  const words = line.words;
  const count = words.length;
  const remove = new Set<number>(edit.remove ?? []);
  const afterValues: string[] = []; // 끼워 넣는 꼴(원문 그대로 — 큰따옴표 등 인용을 지우지 않고 넣는다)
  const afterTexts: string[] = []; // 꼴을 다시 읽은 값 — 뜻한 낱말과 같은지 검사에 쓴다
  for (const value of edit.afterFirst ?? []) {
    const text = singleWordText(value);
    if (text === null) return null;
    afterValues.push(value);
    afterTexts.push(text);
  }
  const appendValues: string[] = [];
  const appendTexts: string[] = [];
  for (const value of edit.append ?? []) {
    const text = singleWordText(value);
    if (text === null) return null;
    appendValues.push(value);
    appendTexts.push(text);
  }
  if (count === 0 && afterTexts.length > 0) return null;
  const { from } = scalarIndexMap(command);
  // 지울 덩어리 [앞 공백 시작, 마지막 낱말 끝) — 유니코드 스칼라 경계
  const cuts: Array<{ lead: number; end: number }> = [];
  let k = 0;
  while (k < count) {
    if (!remove.has(k)) {
      k++;
      continue;
    }
    let last = k;
    while (remove.has(last + 1) && last + 1 < count) last++;
    let lead = words[k].start;
    const floor = k > 0 ? words[k - 1].end : 0;
    const blankBefore = (scalar: number): boolean => {
      const c = command.charCodeAt(from[scalar]);
      return c === 0x20 || c === 0x09 || c === 0x0a;
    };
    if (edit.oneSpace === true) {
      if (lead > floor && blankBefore(lead - 1)) lead--;
    } else {
      while (lead > floor && blankBefore(lead - 1)) lead--;
    }
    cuts.push({ lead, end: words[last].end });
    k = last + 1;
  }
  const insert = afterValues.length === 0 ? "" : " " + afterValues.join(" ");
  let out = "";
  let at = 0;
  if (insert !== "") {
    const firstEnd = words[0].end;
    out += command.slice(from[at], from[firstEnd]);
    out += insert;
    at = firstEnd;
  }
  for (const cut of cuts) {
    if (cut.lead > at) out += command.slice(from[at], from[cut.lead]);
    if (cut.end > at) at = cut.end;
  }
  out += command.slice(from[at]);
  if (appendValues.length > 0) out += " " + appendValues.join(" ");
  const want: string[] = [];
  for (let index = 0; index < count; index++) {
    if (!remove.has(index)) want.push(words[index].text);
  }
  if (afterTexts.length > 0) want.splice(1, 0, ...afterTexts);
  want.push(...appendTexts);
  const reread = shellLine(out);
  if (reread === null || reread.texts.length !== want.length) return null;
  for (let i = 0; i < want.length; i++) {
    if (reread.texts[i] !== want[i]) return null;
  }
  return out;
}

/** claude 명령 한 줄(shellLine 의 것) — 첫 낱말의 원문이 따옴표 없는 claude(경로 꼴 …\claude·claude.exe 도, 대소문자 무시)일 때만.
 * 원문을 보는 이유 — 따옴표 밖 \ 는 값을 읽을 때 풀려 …\claude 의 \ 가 사라지므로(\X → X). 아니면 없다 */
const CLAUDE_FIRST_WORD = /^(?:\S*[\\/])?claude(?:\.exe)?$/i;

export function claudeLine(command: string): ShellLine | null {
  const line = shellLine(command);
  if (line === null || line.words.length === 0) return null;
  if (!CLAUDE_FIRST_WORD.test(line.words[0].raw(command))) return null;
  return line;
}

/** 이어 열기·이어 하기 낱말이 있는지 — --resume·-r·--continue·-c (끝에 = 값을 붙인 꼴도) */
const SESSION_FLAGS = ["--resume", "-r", "--continue", "-c"];

export function hasFlagWord(texts: readonly string[], flags: readonly string[]): boolean {
  for (let i = 1; i < texts.length; i++) {
    for (const flag of flags) {
      if (texts[i] === flag || texts[i].startsWith(flag + "=")) return true;
    }
  }
  return false;
}

/** 새로 띄우는 claude 에 세션 id 를 붙인다(--session-id <newSessionID>, 끝에) — 보드가 그 세션을 미리 안다. 이미 --session-id 가 있으면 빼고
 * 새 id 로. claude 가 아니거나 이어 열기·이어 하기(--resume·-r·--continue·-c)거나 읽지 못하면 (원문, 없다) */
export function withFreshSession(command: string, newSessionID: string): [string, string | null] {
  const line = claudeLine(command);
  if (line === null || hasFlagWord(line.texts, SESSION_FLAGS)) return [command, null];
  const remove = new Set<number>();
  let i = 1;
  while (i < line.texts.length) {
    if (line.texts[i] === "--session-id" && i + 1 < line.texts.length) {
      remove.add(i);
      remove.add(i + 1);
      i += 2;
      continue;
    }
    if (line.texts[i].startsWith("--session-id=")) remove.add(i);
    i++;
  }
  const edited = shellEdit(command, line, { remove: [...remove], oneSpace: true, append: ["--session-id", newSessionID] });
  return edited === null ? [command, null] : [edited, newSessionID];
}

/** 답변 꾸미기(md-render)의 --plugin-dir 낱말 차례 — "--plugin-dir <…\plugins\md-render>" 두 낱말과 "--plugin-dir=…\plugins\md-render" 한 낱말.
 * 첫 낱말(claude)은 보지 않는다. 사용자의 다른 --plugin-dir 은 두고, 묶인 낱말 안의 글자는 낱말 값 전체가 아니면 건드리지 않는다 */
const MD_RENDER_PLUGIN_TAIL = /[\\/]plugins[\\/]md-render[\\/]?$/i;

export function mdRenderArgIndices(texts: readonly string[]): number[] {
  const indices: number[] = [];
  let i = 1;
  while (i < texts.length) {
    if (texts[i] === "--plugin-dir" && i + 1 < texts.length && MD_RENDER_PLUGIN_TAIL.test(texts[i + 1])) {
      indices.push(i, i + 1);
      i += 2;
      continue;
    }
    if (texts[i].startsWith("--plugin-dir=") && MD_RENDER_PLUGIN_TAIL.test(texts[i].slice("--plugin-dir=".length))) indices.push(i);
    i++;
  }
  return indices;
}


