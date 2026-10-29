// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 다른 터미널의 세션을 보드 탭으로 옮기는 순서(mac moveToBoard): 일하는·기다리는 세션만 먼저 묻고, Esc·/exit·Enter 로 끝낸 뒤
// 8초 안에 끝나면 보드 탭에서 이어 열고, 아니면 알림 한 줄. 효과는 가짜 deps 로 받아 호출 순서와 인자를 본다.
// 실행: cd windows/app && node --test src/lib/board/moveToBoard.test.ts   (Node 22.18+/24 는 .ts 를 바로 읽는다)
import assert from "node:assert/strict";
import { test } from "node:test";

import { MOVE_STEPS, EXIT_TIMEOUT_MS, createMoveGuard, moveToBoard, needsConfirm, sessionAtClick } from "./moveToBoard.ts";

const sessionOf = (state: string, over = {}) => ({ id: "sid-1", cwd: "C:\\work\\demo", pid: 4242, pidCtime: 1700000000.5, state, title: "데모 작업", ...over });

// 호출 순서를 log 에 쌓는 가짜 deps — 각 단계의 답을 바꿔 끼운다
function fake(over = {}) {
  const log: unknown[][] = [];
  const deps = {
    confirm: async (title: string, text: string) => (log.push(["confirm", title, text]), true),
    drive: async (id: string, steps: unknown[]) => (log.push(["drive", id, steps]), true),
    waitExit: async (pid: number, pidCtime: number | null, timeoutMs: number) => (log.push(["waitExit", pid, pidCtime, timeoutMs]), true),
    resumeSession: (s: { id: string }) => void log.push(["resumeSession", s.id]),
    toast: (text: string) => void log.push(["toast", text]),
    ...over,
  };
  return { deps, log, names: () => log.map((entry) => entry[0]) };
}

test("키 순서는 Esc, 0.4초, /exit, 0.2초, Enter 다", () => {
  assert.deepEqual(MOVE_STEPS, [{ key: "escape" }, { wait: 400 }, { text: "/exit" }, { wait: 200 }, { key: "enter" }]);
  assert.equal(EXIT_TIMEOUT_MS, 8000);
});

test("일하는·기다리는 세션만 먼저 묻는다", () => {
  const table = { working: true, waiting: true, idle: false, paused: false, ended: false };
  for (const [state, expected] of Object.entries(table)) assert.equal(needsConfirm(sessionOf(state)), expected, state);
});

test("대기 세션은 묻지 않고 끝낸 뒤 이어 연다", async () => {
  const { deps, log, names } = fake();
  await moveToBoard(sessionOf("idle"), deps);
  assert.deepEqual(names(), ["drive", "waitExit", "resumeSession"]);
  assert.deepEqual(log[0], ["drive", "sid-1", MOVE_STEPS]);
  assert.deepEqual(log[1], ["waitExit", 4242, 1700000000.5, 8000]);
  assert.deepEqual(log[2], ["resumeSession", "sid-1"]);
});

test("일하는 세션은 제목과 경고를 담아 묻고, 승인하면 같은 길로 간다", async () => {
  const { deps, log, names } = fake();
  await moveToBoard(sessionOf("working"), deps);
  assert.deepEqual(names(), ["confirm", "drive", "waitExit", "resumeSession"]);
  const [, title, text] = log[0];
  assert.equal(title, "보드로 옮길까요?");
  assert.equal(text, "데모 작업\n\n지금 터미널의 세션을 끝내고 보드 새 탭에서 같은 대화를 이어 엽니다. 진행 중인 응답은 끊깁니다.");
});

test("승인 대기 세션도 묻는다", async () => {
  const { deps, names } = fake();
  await moveToBoard(sessionOf("waiting"), deps);
  assert.equal(names()[0], "confirm");
});

test("확인에서 취소하면 아무것도 넣지 않고 알림도 없다", async () => {
  const { deps, names } = fake({ confirm: async () => false });
  await moveToBoard(sessionOf("working"), deps);
  assert.deepEqual(names(), []);
});

test("세션 터미널을 못 찾으면 폴더 이름과 함께 알리고 더 하지 않는다", async () => {
  const { deps, log, names } = fake({ drive: async () => false });
  await moveToBoard(sessionOf("idle"), deps);
  assert.deepEqual(names(), ["toast"]);
  assert.deepEqual(log[0], ["toast", "demo: 세션 터미널을 찾지 못해 보드로 옮기지 못했습니다"]);
});

test("8초 안에 끝나지 않으면 알리고 이어 열지 않는다", async () => {
  const calls: unknown[][] = [];
  const { deps, log } = fake({ waitExit: async (pid: number, ctime: number | null, ms: number) => (calls.push([pid, ctime, ms]), false) });
  await moveToBoard(sessionOf("idle"), deps);
  assert.deepEqual(calls, [[4242, 1700000000.5, 8000]]);
  assert.deepEqual(log.at(-1), ["toast", "demo: 세션이 끝나지 않아 보드로 옮기지 못했습니다. 그 터미널을 확인해 주세요"]);
  assert.equal(log.filter((entry) => entry[0] === "resumeSession").length, 0);
});

test("이어 열기는 한 번만 부른다", async () => {
  const { deps, log } = fake();
  await moveToBoard(sessionOf("working"), deps);
  assert.equal(log.filter((entry) => entry[0] === "resumeSession").length, 1);
  assert.equal(log.filter((entry) => entry[0] === "toast").length, 0);
});

test("알림의 폴더 이름은 슬래시·역슬래시·끝 구분자에 상관없이 마지막 이름이다", async () => {
  for (const [cwd, name] of [["C:\\work\\demo", "demo"], ["C:/work/demo", "demo"], ["C:\\work\\demo\\", "demo"], ["/home/u/proj/", "proj"], ["demo", "demo"]]) {
    const { deps, log } = fake({ drive: async () => false });
    await moveToBoard(sessionOf("idle", { cwd }), deps);
    assert.equal(log[0][1].split(":")[0], name, cwd);
  }
});

test("pid 를 모르는 세션(0)은 끝났다고 보지 않아 알리고 이어 열지 않는다", async () => {
  const { deps, log } = fake({ waitExit: async (pid: number) => pid !== 0 });
  await moveToBoard(sessionOf("idle", { pid: 0, pidCtime: null }), deps);
  assert.equal(log.filter((entry) => entry[0] === "resumeSession").length, 0);
  assert.match(log.at(-1)[1], /세션이 끝나지 않아/);
});

test("누른 순간의 세션: 지금 상태를 쓰고, 사라졌거나 끝났으면 옮길 것이 없다", () => {
  const stale = sessionOf("idle");
  const now = sessionOf("working");
  assert.equal(sessionAtClick(now), now, "메뉴를 연 뒤 일을 시작했으면 지금 상태(working)로 확인을 거른다");
  assert.equal(needsConfirm(sessionAtClick(now)), true);
  assert.equal(needsConfirm(stale), false);
  assert.equal(sessionAtClick(undefined), null);
  assert.equal(sessionAtClick(sessionOf("ended")), null);
});

test("옮기는 중에 같은 세션을 또 누르면 두 번째는 거른다(키를 두 번 보내지 않는다)", async () => {
  const guard = createMoveGuard();
  let release: () => void = () => {};
  let runs = 0;
  const first = guard("sid-1", () => (runs++, new Promise<void>((resolve) => (release = resolve))));
  assert.equal(await guard("sid-1", async () => void runs++), false, "도는 중이면 시작하지 않는다");
  assert.equal(await guard("sid-2", async () => void runs++), true, "다른 세션은 따로 센다");
  release();
  assert.equal(await first, true);
  assert.equal(runs, 2);
  assert.equal(await guard("sid-1", async () => void runs++), true, "끝나면 다시 옮길 수 있다");
});

test("옮기는 일이 던져도 가드는 풀려 다시 옮길 수 있다", async () => {
  const guard = createMoveGuard();
  await assert.rejects(guard("sid-1", async () => { throw new Error("boom"); }), /boom/);
  assert.equal(await guard("sid-1", async () => {}), true);
});

test("deps 가 던지면 그대로 던진다(조용히 삼키지 않는다)", async () => {
  const boom = fake({ drive: async () => { throw new Error("ipc"); } });
  await assert.rejects(moveToBoard(sessionOf("idle"), boom.deps), /ipc/);
  assert.deepEqual(boom.names(), []);
});
