// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 실행: cd windows/app && node --test src/lib/terminal/shellCommand.test.ts
// mac ShellCommandTests.swift 를 Windows 판(shellCommand.ts)으로 옮긴 것이다 — 기대값의 이름 따옴표는 sessionNameArg 꼴(큰따옴표), 경로는 큰따옴표.
// 공격 줄은 첫 요청 글(인자 하나)에 셸이 다르게 읽을 글자를 넣은 것 — 고친 뒤에는 다시 읽은 낱말이 뜻한 인자 그대로여야 한다.
import { test } from "node:test";
import assert from "node:assert/strict";
import {
  claudeLine,
  hasFlagWord,
  mdRenderArgIndices,
  shellArg,
  shellCommand,
  shellEdit,
  shellLine,
  shellWords,
  shlexQuote,
  singleWordText,
  withFreshSession,
} from "./shellCommand.ts";
import { mdRenderCommand } from "./mdRender.ts";

const DIR = String.raw`C:\Users\me\.claude\notifier\plugins\md-render`;
const NEW = "11111111-2222-3333-4444-555555555555";
const SID = "0f0e1d2c-3b4a-5968-7766-554433221100";

// ── 따옴표 ────────────────────────────────────────────────────────────────────

test("shlexQuote 는 ' 뒤에 결합 문자가 와도 ' 를 바꾼다(옛 정규식은 Character 비교로 지나쳤다)", () => {
  const s = "a'\u{301}; touch /tmp/x; '\u{301}b";
  const q = shlexQuote(s);
  assert.equal(q, "'a'\"'\"'\u{301}; touch /tmp/x; '\"'\"'\u{301}b'");
  assert.deepEqual(shellWords(q), [s]);
  assert.equal(shellLine(q)?.plain, true);
});

test("shellArg 는 맨 앞 = 를 묶는다(셸의 =이름 펼침), 나머지는 shlexQuote 그대로", () => {
  assert.equal(shellArg("=x"), "'=x'");
  assert.equal(shellArg("a=x"), "a=x");
  assert.equal(shellArg("it's"), shlexQuote("it's"));
  assert.equal(shellArg("=a b"), "'=a b'");
});

// ── 낱말 읽기 ─────────────────────────────────────────────────────────────────

test("shellLine 은 낱말 값과 원문 자리를 함께 읽는다", () => {
  const s = `  claude --name 'it'"'"'s' "a \\"b\\" \\$c" d\\ e '' x`;
  const line = shellLine(s);
  if (line === null) throw new Error("반드시 읽혀야 한다");
  assert.deepEqual(line.texts, ["claude", "--name", "it's", `a "b" $c`, "d e", "", "x"]);
  assert.deepEqual(
    line.words.map((word) => word.raw(s)),
    ["claude", "--name", `'it'"'"'s'`, `"a \\"b\\" \\$c"`, `d\\ e`, "''", "x"],
  );
  assert.equal(line.plain, true);
});

// [이름, 줄, 낱말 값(읽지 못하면 null), plain]
const lines = [
  ["닫히지 않은 작은따옴표", "claude '열린", null, false],
  ["닫히지 않은 큰따옴표", 'claude "열린', null, false],
  ["끝의 \\ 는 뒤에 붙일 글자를 삼키므로 실패", "claude x\\", null, false],
  ["\\줄바꿈 은 이어 쓰기(둘 다 없어짐)", "claude a\\\nb \\\n c", ["claude", "ab", "c"], true],
  ["따옴표 밖 ; 는 낱말에 남지만 plain 이 아니다", "claude a;touch x", ["claude", "a;touch", "x"], false],
  ["따옴표 밖 $( ) 도", "claude $(id)", ["claude", "$(id)"], false],
  ["큰따옴표 안 $ 도", `claude "$HOME"`, ["claude", "$HOME"], false],
  ["큰따옴표 안 ! 도(대화형 셸의 기록 펼침)", `claude "a!b"`, ["claude", "a!b"], false],
  ["맨 앞 = (셸의 =이름 펼침)", "claude =ls", ["claude", "=ls"], false],
  ["가운데 = 는 됨", "claude --a=b", ["claude", "--a=b"], true],
  ["따옴표 밖 ~ 는 펼침", "claude ~/x", ["claude", "~/x"], false],
  ["따옴표 밖 줄바꿈은 명령을 나눈다", "claude a\ntouch x", ["claude", "a", "touch", "x"], false],
  ["따옴표 밖 한글은 plain 이 아니다(shlexQuote 는 묶는다)", "claude 한글", ["claude", "한글"], false],
  ["' 뒤 결합 문자: 스칼라로 읽어 ' 를 본다", "claude 'a'\u{301}; x", ["claude", "a\u{301};", "x"], false],
];

for (const [name, line, texts, plain] of lines) {
  test(`shellLine — ${name}`, () => {
    const read = shellLine(line);
    assert.equal(read === null, texts === null, line);
    if (read !== null) {
      assert.deepEqual(read.texts, texts, line);
      assert.equal(read.plain, plain, line);
    }
  });
}

test("shellWords — 읽을 수 있으면 값만, 아니면 없다", () => {
  assert.deepEqual(shellWords("claude --name '역할 1'"), ["claude", "--name", "역할 1"]);
  assert.equal(shellWords("claude '열린"), null);
});

// ── 낱말 단위 고치기(실제 조판: mdRenderCommand · withFreshSession · --name 끼우기) ──

const QUOTED_DIR = `"${DIR}"`; // quotePluginDir 꼴 — 끼울 때는 부르는 쪽이 이미 인용한다
const NARG = '"역할 1"'; // sessionNameArg("역할 1") 꼴

const E = {
  claude: "claude",
  opus: "claude --model opus",
  pathed: String.raw`C:\bin\claude --verbose`,
  spaced: "  claude  --model  opus",
  notClaude: "bash -lc 'claude x'",
  claudex: "claude-x --y",
  oldDir: `claude --resume ${SID} --plugin-dir C:/old/plugins/md-render`,
  oldQuoted: String.raw`claude --model opus --plugin-dir "C:\Users\me\AppData\Local\Programs\Claude Board\plugins\md-render" --resume ${SID}`,
  eqSpaced: `claude '--plugin-dir=/a b/plugins/md-render' --resume ${SID}`,
  eqBare: `claude --plugin-dir=C:/a/plugins/md-render --resume ${SID}`,
  userDir: `claude --plugin-dir C:/my/plugin --resume ${SID}`,
  newSession: `claude --session-id ${SID} --name "역할 2" "첫 요청"`,
  continuing: `claude --model opus --resume ${SID} '이어서 진행해 주세요. 끊긴 곳부터'\\''요'`,
  idPair: `claude --model opus --session-id ${SID}`,
  idEq: `claude --session-id=${SID} --verbose`,
  cont: "claude -c",
  contLong: "claude --continue --model opus",
  resume: "claude -r",
  hasName: `claude --name 역할 --resume ${SID}`,
  nEq: "claude -n=x",
  bareId: "claude --session-id",
  addDir: `claude --add-dir 'a b' c --mcp-config m.json`,
  oldQuotedFwd: `claude --plugin-dir "C:/a/plugins/md-render" --resume ${SID}`,
  unreadable: "claude '열린",
};

// 보드가 이름을 끼우는 꼴(namedCommand 와 같은 조판) — 이름 값 유효검사(isSessionNameArg)는 sessionCommand 쪽에 있어 여기선 묶은 값만 준다
const namedEdit = (input: string): string => {
  const line = claudeLine(input);
  if (line === null || hasFlagWord(line.texts, ["-n", "--name"])) return input;
  return shellEdit(input, line, { afterFirst: ["--name", NARG] }) ?? input;
};

const editRows: Array<[string, string, string, string, string | null, string]> = [
  ["claude 만", E.claude, `claude --plugin-dir ${QUOTED_DIR}`, E.claude, `claude --session-id ${NEW}`, `claude --name ${NARG}`],
  ["옵션 뒤", E.opus, `claude --plugin-dir ${QUOTED_DIR} --model opus`, E.opus, `claude --model opus --session-id ${NEW}`, `claude --name ${NARG} --model opus`],
  ["경로 꼴 claude", E.pathed, String.raw`C:\bin\claude` + ` --plugin-dir ${QUOTED_DIR} --verbose`, E.pathed, E.pathed + ` --session-id ${NEW}`, String.raw`C:\bin\claude` + ` --name ${NARG} --verbose`],
  ["공백 여럿은 그대로", E.spaced, `  claude --plugin-dir ${QUOTED_DIR}  --model  opus`, E.spaced, E.spaced + ` --session-id ${NEW}`, `  claude --name ${NARG}  --model  opus`],
  ["claude 가 아니면 그대로", E.notClaude, E.notClaude, E.notClaude, null, E.notClaude],
  ["claude 로 시작해도 다른 명령", E.claudex, E.claudex, E.claudex, null, E.claudex],
  ["옛 꾸미기 dir 를 지금 dir 로(따옴표 없는 꼴)", E.oldDir, `claude --plugin-dir ${QUOTED_DIR} --resume ${SID}`, `claude --resume ${SID}`, null, `claude --name ${NARG} --resume ${SID} --plugin-dir C:/old/plugins/md-render`],
  ["옛 설치본의 dir(큰따옴표·띄어쓴 경로)", E.oldQuoted, `claude --plugin-dir ${QUOTED_DIR} --model opus --resume ${SID}`, `claude --model opus --resume ${SID}`, null, String.raw`claude --name ${NARG} --model opus --plugin-dir "C:\Users\me\AppData\Local\Programs\Claude Board\plugins\md-render" --resume ${SID}`],
  ["--plugin-dir='…' 꼴(띄어쓴 값은 묶여 있다)", E.eqSpaced, `claude --plugin-dir ${QUOTED_DIR} --resume ${SID}`, `claude --resume ${SID}`, null, `claude --name ${NARG} '--plugin-dir=/a b/plugins/md-render' --resume ${SID}`],
  ["--plugin-dir=… 꼴", E.eqBare, `claude --plugin-dir ${QUOTED_DIR} --resume ${SID}`, `claude --resume ${SID}`, null, `claude --name ${NARG} --plugin-dir=C:/a/plugins/md-render --resume ${SID}`],
  ["사용자의 다른 dir 는 그대로 두고 보드 것도 붙는다", E.userDir, `claude --plugin-dir ${QUOTED_DIR} --plugin-dir C:/my/plugin --resume ${SID}`, E.userDir, null, `claude --name ${NARG} --plugin-dir C:/my/plugin --resume ${SID}`],
  ["새 세션 명령", E.newSession, `claude --plugin-dir ${QUOTED_DIR} --session-id ${SID} --name "역할 2" "첫 요청"`, E.newSession, `claude --name "역할 2" "첫 요청" --session-id ${NEW}`, E.newSession],
  ["이어서 진행(끊긴 곳부터)", E.continuing, `claude --plugin-dir ${QUOTED_DIR} --model opus --resume ${SID} '이어서 진행해 주세요. 끊긴 곳부터'\\''요'`, E.continuing, null, `claude --name ${NARG} --model opus --resume ${SID} '이어서 진행해 주세요. 끊긴 곳부터'\\''요'`],
  ["--session-id 를 새것으로(값 꼴)", E.idPair, `claude --plugin-dir ${QUOTED_DIR} --model opus --session-id ${SID}`, E.idPair, `claude --model opus --session-id ${NEW}`, `claude --name ${NARG} --model opus --session-id ${SID}`],
  ["--session-id= 를 새것으로(= 꼴)", E.idEq, `claude --plugin-dir ${QUOTED_DIR} --session-id=${SID} --verbose`, E.idEq, `claude --verbose --session-id ${NEW}`, `claude --name ${NARG} --session-id=${SID} --verbose`],
  ["-c", E.cont, `claude --plugin-dir ${QUOTED_DIR} -c`, E.cont, null, `claude --name ${NARG} -c`],
  ["--continue", E.contLong, `claude --plugin-dir ${QUOTED_DIR} --continue --model opus`, E.contLong, null, `claude --name ${NARG} --continue --model opus`],
  ["-r", E.resume, `claude --plugin-dir ${QUOTED_DIR} -r`, E.resume, null, `claude --name ${NARG} -r`],
  ["이미 --name 이 있으면 그대로", E.hasName, `claude --plugin-dir ${QUOTED_DIR} --name 역할 --resume ${SID}`, E.hasName, null, E.hasName],
  ["-n=x", E.nEq, `claude --plugin-dir ${QUOTED_DIR} -n=x`, E.nEq, `claude -n=x --session-id ${NEW}`, E.nEq],
  ["값 없는 --session-id 는 새 id 를 뒤에 붙인다", E.bareId, `claude --plugin-dir ${QUOTED_DIR} --session-id`, E.bareId, `claude --session-id --session-id ${NEW}`, `claude --name ${NARG} --session-id`],
  ["묶인 --add-dir 는 끼지 않는다", E.addDir, `claude --plugin-dir ${QUOTED_DIR} --add-dir 'a b' c --mcp-config m.json`, E.addDir, E.addDir + ` --session-id ${NEW}`, `claude --name ${NARG} --add-dir 'a b' c --mcp-config m.json`],
  ["옛 꾸미기 dir(큰따옴표 꼴)", E.oldQuotedFwd, `claude --plugin-dir ${QUOTED_DIR} --resume ${SID}`, `claude --resume ${SID}`, null, `claude --name ${NARG} --plugin-dir "C:/a/plugins/md-render" --resume ${SID}`],
  ["읽지 못하는 명령은 그대로", E.unreadable, E.unreadable, E.unreadable, null, E.unreadable],
];

for (const [name, input, md, off, fresh, named] of editRows) {
  test(`고치기 — ${name}`, () => {
    assert.equal(mdRenderCommand(input, DIR, false), md, input);
    assert.equal(mdRenderCommand(input, DIR, true), off, input);
    assert.deepEqual(withFreshSession(input, NEW), fresh === null ? [input, null] : [fresh, NEW], input);
    assert.equal(namedEdit(input), named, input);
  });
}

// mdRender.test.ts 표의 30번째 줄(`...MD-Render\"`)과 같은 기대다(3.4.0: 읽지 못하면 그대로). cmd argv 에서 `\"` 는
// 이스케이프된 큰따옴표라 묶음이 닫히지 않고(CommandLineToArgvW 도 `\"` 에서 인자를 이어 붙인다), 낱말 읽기는 실패를 고쳐 읽지
// 않는다 — 고칠 수 없는 명령은 그대로 둔다.
test('mdRender.test 의 30번째 줄 — `\\"` 로 끝나는 옛 dir 명령은 읽지 못하므로 그대로 둔다(표 30번째 줄과 같은 기대)', () => {
  const input = String.raw`claude --plugin-dir "C:\X\Plugins\MD-Render\" --resume abc`;
  assert.equal(shellLine(input), null);
  assert.equal(mdRenderCommand(input, DIR, false), input);
});

// ── 공격 줄 — 첫 요청 글에 셸이 다르게 읽을 글자를 넣어도 낱말이 깨지지 않는다 ──

const PWN = "; touch /tmp/cb-pwned #";

const attackRows: Array<[string, string[], boolean]> = [
  ["첫 요청 글 안의 --plugin-dir '…/plugins/md-render'(따옴표 꼴) — 남아 있으면 또 붙이지 않는다",
    ["claude", "--session-id", SID, `x --plugin-dir '/x/plugins/md-render' ${PWN}`], false],
  ["첫 요청 글 안의 --plugin-dir …/plugins/md-render(따옴표 없는 꼴)",
    ["claude", "--session-id", SID, `보세요 --plugin-dir /x/plugins/md-render 끝`], false],
  [`'"'"' 꼴을 글로`, ["claude", "--session-id", SID, `a '"'"' --plugin-dir='/x/plugins/md-render' b`], false],
  ["글 끝의 --session-id 값이 닫는 따옴표를 삼키게(3.3.2 되살리기에서 다음 인자가 명령이 됨)",
    ["claude", "--session-id", SID, "요약해 줘 --session-id x", PWN], true],
  ["--session-id= 꼴", ["claude", "--session-id", SID, "a --session-id=x", PWN], true],
  ["--name 글", ["claude", "--session-id", SID, "use -n flag --name 'x' ; touch /tmp/cb-pwned"], true],
  ["; 와 $(…)", ["claude", "--session-id", SID, "$(touch /tmp/cb-pwned); `id` && echo"], true],
  ["' 뒤 결합 문자(3.3.2 shlexQuote 가 ' 를 못 바꿈)", ["claude", "--session-id", SID, `요약 '\u{301}; touch /tmp/cb-pwned; : '\u{301}`], true],
  ["--resume 글은 이어 열기가 아니다", ["claude", "--session-id", SID, "--resume 하지 마 -c"], true],
  ["줄바꿈·탭이 든 글", ["claude", "--session-id", SID, "첫 줄\n; touch /tmp/cb-pwned\t#"], true],
];

for (const [name, argv, mdAppends] of attackRows) {
  test(`공격 — ${name}`, () => {
    const typed = shellCommand(argv);
    if (typed === null) throw new Error("명령을 만들지 못했다: " + JSON.stringify(argv));
    const line = shellLine(typed);
    if (line === null) throw new Error("명령을 읽지 못했다: " + typed);
    assert.equal(line.plain, true, typed);
    assert.deepEqual(line.texts, argv, typed);
    const md = mdRenderCommand(typed, DIR, false);
    const mdLine = shellLine(md);
    if (mdLine === null) throw new Error("꾸민 명령을 읽지 못했다: " + md);
    assert.deepEqual(mdLine.texts, mdAppends ? [argv[0], "--plugin-dir", DIR, ...argv.slice(1)] : argv, md);
    const off = mdRenderCommand(typed, DIR, true);
    assert.deepEqual(shellLine(off)?.texts ?? null, argv, off);
    const named = shellEdit(typed, line, { afterFirst: ["--name", NARG] });
    if (named === null) throw new Error("이름을 끼우지 못했다: " + typed);
    assert.deepEqual(shellLine(named)?.texts, ["claude", "--name", "역할 1", ...argv.slice(1)], named);
    const fresh = withFreshSession(typed, NEW);
    assert.equal(fresh[1], NEW, typed);
    const fLine = shellLine(fresh[0]);
    if (fLine === null) throw new Error("새 세션 명령을 읽지 못했다: " + fresh[0]);
    assert.deepEqual(fLine.texts, ["claude", ...argv.slice(3), "--session-id", NEW], fresh[0]);
    assert.equal(fLine.plain, true, fresh[0]);
  });
}

test("바깥 열기: 요청의 md-render --plugin-dir 은 빼고 보드 경로로(사용자의 다른 dir 은 그대로)", () => {
  const argv = ["claude", "--plugin-dir", "/old/plugins/md-render", "--plugin-dir=/b/plugins/md-render", "--plugin-dir", "/mine", "글"];
  const typed = shellCommand(argv);
  if (typed === null) throw new Error("명령을 만들지 못했다");
  assert.equal(mdRenderCommand(typed, DIR, false), `claude --plugin-dir ${QUOTED_DIR} --plugin-dir /mine '글'`);
  assert.equal(mdRenderCommand(typed, DIR, true), `claude --plugin-dir /mine '글'`);
});

// ── unit — shellCommand · singleWordText · shellEdit · claudeLine · 깃발 판정 ──

test("shellCommand — 만든 명령은 읽었을 때 같은 인자여야 한다", () => {
  assert.equal(shellCommand([]), "");
  assert.equal(shellCommand(["claude"]), "claude");
  assert.equal(shellCommand(["claude", "a b"]), "claude 'a b'");
  assert.equal(shellCommand(["claude", "=x"]), "claude '=x'");
  assert.equal(shellCommand(["claude", "it's"]), `claude 'it'"'"'s'`);
  assert.equal(shellCommand(["claude", "한글"]), "claude '한글'");
  assert.equal(shellCommand(["claude", "a\nb"]), "claude 'a\nb'");
});

test("singleWordText — 낱말 하나 값만 있다", () => {
  assert.equal(singleWordText(`"역할 1"`), "역할 1");
  assert.equal(singleWordText(`'a'b`), "ab");
  assert.equal(singleWordText(`"a "b"`), null);
  assert.equal(singleWordText("claude x"), null);
  assert.equal(singleWordText("''"), "");
  assert.equal(singleWordText(""), null);
});

test("shellEdit — 끼우는 값은 낱말 하나로 다시 읽혀야 한다", () => {
  const line = shellLine("claude x y");
  if (line === null) throw new Error("읽지 못했다");
  assert.equal(shellEdit("claude x y", line, { afterFirst: ["a b"] }), null);       // 그대로 넣으면 두 낱말이 된다
  assert.equal(shellEdit("claude x y", line, { afterFirst: [`"a b"`] }), `claude "a b" x y`);
  assert.equal(shellEdit("claude x y", line, { afterFirst: ["'열린"] }), null);      // 읽을 수 없는 값
  assert.equal(shellEdit("claude x y", line, { afterFirst: ["x\ny"] }), null);       // 개행은 낱말을 나눈다
  assert.equal(shellEdit("claude", shellLine("claude"), { append: [`'열린`] }), null);
  assert.equal(shellEdit("", shellLine(""), { append: ["x"] }), " x");              // 빈 명령에도 끝에 붙는다
  assert.equal(shellEdit("", shellLine(""), { afterFirst: ["x"] }), null);          // 끼울 자리가 없다
});

test("shellEdit — 이어진 차례는 한 덩어리로, 떨어진 차례는 따로 지운다", () => {
  const line = shellLine("claude --a 1 --b 2 --c 3");
  if (line === null) throw new Error("읽지 못했다");
  assert.equal(shellEdit("claude --a 1 --b 2 --c 3", line, { remove: [1, 2, 3] }), "claude 2 --c 3");
  assert.equal(shellEdit("claude --a 1 --b 2 --c 3", line, { remove: [1, 3] }), "claude 1 2 --c 3");
  assert.equal(shellEdit("claude --a 1 --b 2 --c 3", line, { remove: [4, 5, 6] }), "claude --a 1 --b");
  assert.equal(shellEdit("claude --a 1 --b 2 --c 3", line, { remove: [1, 2, 3, 4, 5, 6] }), "claude");
});

test("새 세션 id 로 바꿀 때는 지우는 낱말 앞의 공백을 한 글자만 걷는다", () => {
  assert.deepEqual(
    withFreshSession(String.raw`claude --x  --session-id  old`, NEW),
    [`claude --x  --session-id ${NEW}`, NEW],
  );
});

test("claudeLine — 원문 첫 낱말이 claude 꼴일 때만(claudex·묶은 claude 는 아니다)", () => {
  assert.deepEqual(claudeLine("claude --model opus")?.texts, ["claude", "--model", "opus"]);
  assert.notEqual(claudeLine(String.raw`C:\bin\claude.exe --verbose`), null);
  assert.notEqual(claudeLine(String.raw`.\claude -r`), null);
  assert.equal(claudeLine("bash -lc 'claude x'"), null);
  assert.equal(claudeLine("claude-x --y"), null);
  assert.equal(claudeLine("claudex"), null);
  assert.equal(claudeLine('"claude" --y'), null);
  assert.equal(claudeLine("claude '열린"), null);
});

test("mdRenderArgIndices — 값 꼴과 = 꼴의 차례를 낱말로 내놓는다", () => {
  assert.deepEqual(
    mdRenderArgIndices(["claude", "--plugin-dir", "C:/a/plugins/md-render/", "--plugin-dir=C:/b/plugins/md-render", "x"]),
    [1, 2, 3],
  );
  // 깃발 글자는 정확히 --plugin-dir 이어야 한다(mac mdRenderArgIndices 와 같다; 꼬리는 백슬래시·대소문자·끝 슬래시도 본다 — Windows 어댑터차)
  assert.deepEqual(mdRenderArgIndices(["claude", "--Plugin-Dir=C:/b/plugins/md-render"]), []);
  assert.deepEqual(mdRenderArgIndices(["claude", "--plugin-dir", "/mine", "--plugin-dir", "C:/x/plugins/md-render-fork"]), []);
  assert.deepEqual(mdRenderArgIndices(["--plugin-dir", "C:/a/plugins/md-render"]), []);
});

test("hasFlagWord — 낱말 하나가 통째로 그 깃발일 때만", () => {
  assert.equal(hasFlagWord(["claude", "--resume", "x"], ["--resume"]), true);
  assert.equal(hasFlagWord(["claude", "--resume=x"], ["--resume"]), true);
  assert.equal(hasFlagWord(["claude", "-c"], ["--continue", "-c"]), true);
  assert.equal(
    hasFlagWord(["claude", "--session-id", SID, "--resume 하지 마 -c"], ["--resume", "-r", "--continue", "-c"]),
    false,
  );
});
