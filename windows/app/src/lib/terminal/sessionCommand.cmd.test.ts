// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 실행: cd windows/app && node --test src/lib/terminal/sessionCommand.cmd.test.ts   (Windows 에서만 돈다)
// 보드는 이 줄을 셸 탭에 그대로 쳐 넣고 Enter 를 보낸다. cmd 탭에서도 줄이 한 명령이고 --name 이 argv 한 칸으로 가는지 잰다.
// claude.exe 대신 PATH 앞에 둔 claude.cmd 가 받은 인자를 node 로 JSON 에 남긴다(이름이 허용 글자뿐이라 %* 로 다시 풀어도 같다).
// 순수 함수 시험은 sessionCommand.test.ts 에 있다.
import { test } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { freshSessionCommand, namedCommand } from "./sessionCommand.ts";
import { sanitizeSessionName, sessionNameArg } from "./sessionName.ts";
import { hostileFolders, newSession } from "./sessionCommand.fixtures.ts";

const onWindows = process.platform === "win32";

function cmdSandbox() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "claude-board-cmd-"));
  fs.writeFileSync(
    path.join(dir, "dump.cjs"),
    "require('fs').appendFileSync(process.env.ARGV_OUT, JSON.stringify(process.argv.slice(2)) + '\\n');",
  );
  fs.writeFileSync(path.join(dir, "claude.cmd"), `@"${process.execPath}" "%~dp0dump.cjs" %*\r\n`);
  const argvOut = path.join(dir, "argv.jsonl");
  const canary = path.join(dir, "canary.txt");
  const runLine = (line) => {
    fs.rmSync(argvOut, { force: true });
    fs.rmSync(canary, { force: true });
    const comspec = process.env.ComSpec || "cmd.exe";
    const result = spawnSync(comspec, ["/d", "/s", "/c", `"${line}"`], {
      cwd: dir,
      env: { ...process.env, PATH: dir + path.delimiter + process.env.PATH, ARGV_OUT: argvOut, CANARY: canary },
      windowsVerbatimArguments: true,
      encoding: "utf8",
    });
    const calls = fs.existsSync(argvOut) ? fs.readFileSync(argvOut, "utf8").trim().split("\n").map((row) => JSON.parse(row)) : [];
    return { calls, canary: fs.existsSync(canary), status: result.status };
  };
  return { dir, runLine };
}

test("cmd.exe — 어떤 폴더 이름이든 claude 는 한 번만 불리고 argv 는 정확히 다섯 칸", { skip: !onWindows }, () => {
  const sandbox = cmdSandbox();
  try {
    for (const folder of hostileFolders) {
      const { line, sessionId } = newSession(folder);
      const run = sandbox.runLine(line);
      assert.deepEqual(
        run.calls,
        [["--name", sanitizeSessionName(folder), "--dangerously-skip-permissions", "--session-id", sessionId]],
        JSON.stringify(folder),
      );
      assert.equal(run.canary, false, folder);
    }
  } finally {
    fs.rmSync(sandbox.dir, { recursive: true, force: true });
  }
});

test("cmd.exe — 이어 열기 줄과 실어 보내는 인자도 argv 한 칸씩", { skip: !onWindows }, () => {
  const sandbox = cmdSandbox();
  try {
    const line = namedCommand(freshSessionCommand('claude --resume abc --append-system-prompt "hello world" --model m').command, sessionNameArg("my app"));
    assert.deepEqual(sandbox.runLine(line).calls, [
      ["--name", "my app", "--resume", "abc", "--append-system-prompt", "hello world", "--model", "m"],
    ]);
  } finally {
    fs.rmSync(sandbox.dir, { recursive: true, force: true });
  }
});

test("cmd.exe — 대조군: 정리하지 않은 이름은 두 번째 명령을 실행한다(이 시험이 주입을 잡는다는 증거)", { skip: !onWindows }, () => {
  const sandbox = cmdSandbox();
  try {
    assert.equal(sandbox.runLine('claude --name a & echo x> "%CANARY%"').canary, true);
    assert.equal(sandbox.runLine("claude --name a & claude --name second").calls.length, 2);
  } finally {
    fs.rmSync(sandbox.dir, { recursive: true, force: true });
  }
});
