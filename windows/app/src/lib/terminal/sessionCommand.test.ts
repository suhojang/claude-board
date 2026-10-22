// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 실행: cd windows/app && node --test src/lib/terminal/sessionCommand.test.ts   (Node 22.18+/24 는 .ts 를 바로 읽는다)
// 실제 cmd.exe 에 주는 시험은 sessionCommand.cmd.test.ts 에 있다.
import { test } from "node:test";
import assert from "node:assert/strict";
import { freshSessionCommand, namedCommand } from "./sessionCommand.ts";
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
