// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 실행: cd windows/app && node --test src/lib/terminal/sessionCommand.test.ts   (Node 22.18+/24 는 .ts 를 바로 읽는다)
// 실제 cmd.exe 에 주는 시험은 sessionCommand.cmd.test.ts 에 있다.
import { test } from "node:test";
import assert from "node:assert/strict";
import { CARRY_ON_PROMPT, freshSessionCommand, namedCommand, resumeCommand } from "./sessionCommand.ts";
import { sessionNameArg } from "./sessionName.ts";
import { hostileFolders, newSession } from "./sessionCommand.fixtures.ts";

const UUID = "[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}";
const NAME = String.raw`("[\p{L}\p{N} _.-]+"|[\p{L}\p{N}_.-]+)`;
const SHAPE = new RegExp(`^claude --name ${NAME} --dangerously-skip-permissions --session-id ${UUID}$`, "u");

test("새 세션 줄 모양 — 이름은 claude 바로 뒤, 공백이 있으면 큰따옴표", () => {
  const { line, sessionId } = newSession("shop-api");
  assert.equal(line, `claude --name shop-api --dangerously-skip-permissions --session-id ${sessionId}`);
  assert.equal(newSession("my app").line.split(" --dangerously")[0], 'claude --name "my app"');
  assert.equal(newSession("내 프로젝트").line.split(" --dangerously")[0], 'claude --name "내 프로젝트"');
});

test("어떤 폴더 이름이든 줄은 한 모양 — 이름 밖에는 셸 특수문자가 없다", () => {
  for (const folder of hostileFolders) assert.match(newSession(folder).line, SHAPE, JSON.stringify(folder));
});

test("이어 열기·이어 하기는 세션 id 를 붙이지 않고, --name 이 있으면 이름도 붙이지 않는다", () => {
  const san = (command) => namedCommand(freshSessionCommand(command).command, sessionNameArg("proj"));
  assert.equal(san("claude --resume abc"), "claude --name proj --resume abc");
  assert.equal(san("claude -r abc --model x"), "claude --name proj -r abc --model x");
  assert.equal(san("claude --resume=abc"), "claude --name proj --resume=abc");
  assert.equal(san("claude --continue"), "claude --name proj --continue");
  assert.equal(san("claude --name foo --resume x"), "claude --name foo --resume x");
  assert.equal(san("claude-x --foo"), "claude-x --foo");
});

test("실어 보내는 인자는 그대로 두고 옛 --session-id 만 새 id 로 바꾼다", () => {
  const line = namedCommand(freshSessionCommand("claude --session-id old-id --model claude-opus-5-5").command, sessionNameArg("proj"));
  assert.match(line, new RegExp(`^claude --name proj --model claude-opus-5-5 --session-id ${UUID}$`));
});

test("namedCommand 는 정리하지 않은 이름을 명령줄에 넣지 않는다", () => {
  for (const raw of ["a & calc", 'a"b', "%PATH%", "a^b", "-n", "", "a\r\nb"]) {
    assert.throws(() => namedCommand("claude --model m", raw), raw);
  }
  assert.equal(namedCommand("notclaude --x", "a & calc"), "notclaude --x");
});

const ID = "8f3c2a10-1111-4222-8333-444455556666";

// state.rs 가 기록하는 실제 모양 — resumeArgs 는 늘 `--resume <id>` 로 끝난다
const real = (args) => [...args, "--resume", ID];

test("resumeCommand — 기본은 인자 없이 claude --resume <id>", () => {
  assert.equal(resumeCommand(real([]), ID, false), `claude --resume ${ID}`);
  assert.equal(resumeCommand([], ID, false), `claude --resume ${ID}`);
});

test("resumeCommand — 허용 글자만 든 인자는 그대로 남긴다", () => {
  assert.equal(
    resumeCommand(real(["--model", "claude-opus-5-5", "--dangerously-skip-permissions"]), ID, false),
    `claude --resume ${ID} --model claude-opus-5-5 --dangerously-skip-permissions`,
  );
  assert.equal(resumeCommand(real(["--permission-mode=plan"]), ID, false), `claude --resume ${ID} --permission-mode=plan`);
});

test("resumeCommand — -p·--print 는 뺀다", () => {
  assert.equal(resumeCommand(real(["-p", "--model", "m"]), ID, false), `claude --resume ${ID} --model m`);
  assert.equal(resumeCommand(real(["--print"]), ID, false), `claude --resume ${ID}`);
});

test("resumeCommand — 허용 밖 글자가 든 인자가 하나라도 있으면 인자를 모두 버린다", () => {
  for (const bad of ["a b", "a&b", "100%", 'a"b', "$HOME", "a;b", "a`b", "한글", "a^b", "a!b", "", "a\nb"]) {
    assert.equal(resumeCommand(real(["--model", "ok", bad]), ID, false), `claude --resume ${ID}`, JSON.stringify(bad));
  }
});

test("resumeCommand — 이어 열기와 부딪히는 인자(--resume·--name 따위)도 기본 줄로 돌아간다", () => {
  for (const flag of ["--resume", "-r", "--continue", "-c", "--session-id", "--name", "-n", "--name=x", "--resume=abc"]) {
    assert.equal(resumeCommand(real([flag, "--model", "m"]), ID, false), `claude --resume ${ID}`, flag);
  }
});

test("resumeCommand — 끝의 --resume <id> 쌍은 떼고 앞의 --model·권한 인자는 살린다", () => {
  const args = ["--model", "claude-opus-5-5", "--permission-mode", "plan", "--dangerously-skip-permissions", "--resume", ID];
  assert.equal(
    resumeCommand(args, ID, false),
    `claude --resume ${ID} --model claude-opus-5-5 --permission-mode plan --dangerously-skip-permissions`,
  );
});

test("resumeCommand — 맨 앞이 @ 인 인자는 파일 첨부로 읽히니 인자를 모두 버린다", () => {
  for (const bad of ["@secret.txt", "@C:/x"]) {
    assert.equal(resumeCommand(real(["--model", "m", bad]), ID, false), `claude --resume ${ID}`, bad);
  }
  assert.equal(resumeCommand(real(["--model", "a@b"]), ID, false), `claude --resume ${ID} --model a@b`);
});

test("resumeCommand — 세션 id 모양이 틀리면 던진다", () => {
  for (const bad of ["", "short", "a b c d e f g h", "abc&def-ghi", `${ID}x"`, "x".repeat(65), "한글한글한글한글한글"]) {
    assert.throws(() => resumeCommand([], bad, false), JSON.stringify(bad));
  }
});

test("resumeCommand — carryOn 이면 큰따옴표로 묶은 재개 문구를 뒤에 붙인다", () => {
  assert.equal(resumeCommand([], ID, true), `claude --resume ${ID} "${CARRY_ON_PROMPT}"`);
  assert.equal(resumeCommand(["--model", "m"], ID, true), `claude --resume ${ID} --model m "${CARRY_ON_PROMPT}"`);
});

test("CARRY_ON_PROMPT — 큰따옴표 안에서 풀리는 셸 특수문자가 없다", () => {
  assert.doesNotMatch(CARRY_ON_PROMPT, /["$%!`^&\\\r\n]/);
  assert.ok(CARRY_ON_PROMPT.length > 0);
});

test("resumeCommand 결과에 namedCommand 가 --name 을 claude 바로 뒤에 붙인다", () => {
  assert.equal(namedCommand(resumeCommand([], ID, false), sessionNameArg("proj")), `claude --name proj --resume ${ID}`);
});

test("resumeCommand — 세션 id 길이 경계: 8자·64자는 되고 7자·65자는 던진다", () => {
  assert.equal(resumeCommand([], "abcdefgh", false), "claude --resume abcdefgh");
  assert.equal(resumeCommand([], "a".repeat(64), false), `claude --resume ${"a".repeat(64)}`);
  assert.throws(() => resumeCommand([], "abcdefg", false));
  assert.throws(() => resumeCommand([], "a".repeat(65), false));
});

test("resumeCommand — 끝에 줄바꿈이 붙은 id·인자는 허용하지 않는다($ 가 줄바꿈 앞에서 맞는 함정)", () => {
  assert.throws(() => resumeCommand([], `${ID}\n`, false));
  assert.throws(() => resumeCommand([], `${ID}\r`, false));
  assert.equal(resumeCommand(["--model\n"], ID, false), `claude --resume ${ID}`);
  assert.equal(resumeCommand(["--model", "m\r"], ID, false), `claude --resume ${ID}`);
});

test("resumeCommand — 전각·유니코드 비슷한 글자와 NUL 은 허용 밖이다", () => {
  for (const bad of ["a＆b", "a；b", "ａｂ", "a\u0000b", "a\u00a0b", "a\u200bb", "—model"]) {
    assert.equal(resumeCommand(["--model", bad], ID, false), `claude --resume ${ID}`, JSON.stringify(bad));
  }
});

test("resumeCommand — 경로 구분자 \\ 가 든 인자는 허용 밖(cmd 이스케이프 혼동)이고 / 는 허용", () => {
  assert.equal(resumeCommand(["--add-dir=C:\\x"], ID, false), `claude --resume ${ID}`);
  assert.equal(resumeCommand(["--add-dir=C:/x"], ID, false), `claude --resume ${ID} --add-dir=C:/x`);
});

test("resumeCommand — -p 가 여럿이어도 모두 빠지고 나머지 순서는 유지된다, 입력 배열은 그대로", () => {
  const args = ["-p", "--model", "m", "--print", "-p", "--verbose"];
  const copy = [...args];
  assert.equal(resumeCommand(args, ID, false), `claude --resume ${ID} --model m --verbose`);
  assert.deepEqual(args, copy);
});

test("resumeCommand — -p 와 허용 밖 인자가 함께 있으면 -p 도 버려진 기본 줄이다", () => {
  assert.equal(resumeCommand(["-p", "a b"], ID, false), `claude --resume ${ID}`);
});

test("resumeCommand — 값 없는 단독 하이픈·=·점 인자는 모양만 맞으면 그대로 간다(의미는 claude 가 판단)", () => {
  assert.equal(resumeCommand(["-", "=", "."], ID, false), `claude --resume ${ID} - = .`);
});

test("resumeCommand — 대소문자만 다른 충돌 인자(--Resume·-R)는 다른 인자라 그대로 남는다", () => {
  assert.equal(resumeCommand(["--Resume", "-R"], ID, false), `claude --resume ${ID} --Resume -R`);
});

test("resumeCommand — 접두가 비슷한 인자(--name-x·--resumed·-nx)는 충돌이 아니다", () => {
  assert.equal(resumeCommand(["--resumed", "--name-x", "-nx"], ID, false), `claude --resume ${ID} --resumed --name-x -nx`);
});

test("resumeCommand — carryOn 줄에는 큰따옴표가 정확히 둘이고 문구는 맨 끝이다", () => {
  const line = resumeCommand(["--model", "m"], ID, true);
  assert.equal((line.match(/"/g) ?? []).length, 2);
  assert.ok(line.endsWith(`"${CARRY_ON_PROMPT}"`));
  assert.equal(line.indexOf('"'), line.lastIndexOf(`"${CARRY_ON_PROMPT}"`));
});

test("resumeCommand — 허용 밖 인자가 있어도 carryOn 문구는 기본 줄 뒤에 붙는다", () => {
  assert.equal(resumeCommand(["a b"], ID, true), `claude --resume ${ID} "${CARRY_ON_PROMPT}"`);
});

test("resumeCommand + namedCommand — 문구가 든 줄에도 --name 은 claude 바로 뒤에 한 번만 붙는다", () => {
  const line = namedCommand(resumeCommand([], ID, true), sessionNameArg("proj"));
  assert.equal(line, `claude --name proj --resume ${ID} "${CARRY_ON_PROMPT}"`);
  assert.equal(line.match(/--name/g).length, 1);
});

test("resumeCommand — 어떤 입력에서도 결과에 개행·셸 메타문자가 큰따옴표 밖에 나오지 않는다", () => {
  const nasty = ["a&b", "a|b", "a^b", "%x%", "!x!", "a;b", "a`b", "$x", "a>b", "a<b", "a\r\nb", "(x)", "한글", "a b"];
  for (const bad of nasty) {
    for (const carryOn of [false, true]) {
      const line = resumeCommand(["--model", bad], ID, carryOn);
      const outsideQuotes = line.replace(/"[^"]*"/g, "");
      assert.doesNotMatch(outsideQuotes, /[&|^%!;`$<>()\r\n]/u, `${JSON.stringify(bad)} carryOn=${carryOn}`);
    }
  }
});
