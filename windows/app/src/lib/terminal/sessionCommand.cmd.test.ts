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
import { CARRY_ON_PROMPT, freshSessionCommand, namedCommand, resumeCommand } from "./sessionCommand.ts";
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

// ── resumeCommand — 이어 열기 줄을 실제 cmd.exe·PowerShell 이 읽는 대로 ────────────────────────────

const RESUME_ID = "8f3c2a10-1111-4222-8333-444455556666";
// 허용 밖이라 resumeCommand 가 인자를 모두 버리게 만드는 메타문자 인자들(cmd.exe·PowerShell 양쪽의 특수 글자)
const metaArgs = [
  "a&calc", "a|b", "a^b", "%COMSPEC%", "!COMSPEC!", 'a"b', "a b", "a;calc", "a`b", "$env:PATH", "$(calc)", "a>b", "a<b",
  "a,b", "(x)", "{x}", "a'b", "한글", "a\\b", "~", "*", "?", "#", "a\r", "a\n",
];

test("cmd.exe — 이어 열기 줄은 메타문자 인자를 모두 버려 argv 가 --resume <id> 둘뿐이고 주입이 없다", { skip: !onWindows }, () => {
  const sandbox = cmdSandbox();
  try {
    for (const bad of metaArgs) {
      const line = resumeCommand(["--model", "ok", bad], RESUME_ID, false);
      assert.equal(line, `claude --resume ${RESUME_ID}`, JSON.stringify(bad));
      const run = sandbox.runLine(namedCommand(line, sessionNameArg("proj")));
      assert.deepEqual(run.calls, [["--name", "proj", "--resume", RESUME_ID]], JSON.stringify(bad));
      assert.equal(run.canary, false, JSON.stringify(bad));
    }
  } finally {
    fs.rmSync(sandbox.dir, { recursive: true, force: true });
  }
});

test("cmd.exe — 허용 인자는 argv 한 칸씩 가고 -p 는 빠진다", { skip: !onWindows }, () => {
  const sandbox = cmdSandbox();
  try {
    const line = resumeCommand(["-p", "--model", "claude-opus-5-5", "--permission-mode=plan", "--add-dir=C:/x/y", "--dangerously-skip-permissions"], RESUME_ID, false);
    assert.deepEqual(sandbox.runLine(line).calls, [
      ["--resume", RESUME_ID, "--model", "claude-opus-5-5", "--permission-mode=plan", "--add-dir=C:/x/y", "--dangerously-skip-permissions"],
    ]);
  } finally {
    fs.rmSync(sandbox.dir, { recursive: true, force: true });
  }
});

test("cmd.exe — carryOn 문구는 큰따옴표 안에서 argv 마지막 한 칸으로 간다", { skip: !onWindows }, () => {
  const sandbox = cmdSandbox();
  try {
    for (const args of [[], ["--model", "m"], ["-p"]]) {
      const line = namedCommand(resumeCommand(args, RESUME_ID, true), sessionNameArg("my app"));
      const run = sandbox.runLine(line);
      const expectedArgs = args.filter((arg) => arg !== "-p");
      assert.deepEqual(run.calls, [["--name", "my app", "--resume", RESUME_ID, ...expectedArgs, CARRY_ON_PROMPT]], JSON.stringify(args));
      assert.equal(run.canary, false);
    }
  } finally {
    fs.rmSync(sandbox.dir, { recursive: true, force: true });
  }
});

test("cmd.exe — 대조군: 걸러 내지 않은 메타문자 인자는 실제로 두 번째 명령을 실행한다", { skip: !onWindows }, () => {
  const sandbox = cmdSandbox();
  try {
    assert.equal(sandbox.runLine(`claude --resume ${RESUME_ID} a&echo x> "%CANARY%"`).canary, true);
  } finally {
    fs.rmSync(sandbox.dir, { recursive: true, force: true });
  }
});

// Windows PowerShell 5.1 은 어느 Windows 에나 있다. 같은 claude.cmd 를 PowerShell 이 부르게 해 같은 argv 가 나오는지 본다.
function powershellRun(sandbox, line) {
  const argvOut = path.join(sandbox.dir, "ps-argv.jsonl");
  fs.rmSync(argvOut, { force: true });
  const result = spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", line], {
    cwd: sandbox.dir,
    env: { ...process.env, PATH: sandbox.dir + path.delimiter + process.env.PATH, ARGV_OUT: argvOut, CANARY: path.join(sandbox.dir, "ps-canary.txt") },
    encoding: "utf8",
  });
  const calls = fs.existsSync(argvOut) ? fs.readFileSync(argvOut, "utf8").trim().split("\n").map((row) => JSON.parse(row)) : [];
  return { calls, canary: fs.existsSync(path.join(sandbox.dir, "ps-canary.txt")), status: result.status, stderr: result.stderr };
}

test("PowerShell — 이어 열기 줄: 허용 인자는 한 칸씩, 메타문자 인자는 버려져 주입이 없다", { skip: !onWindows }, () => {
  const sandbox = cmdSandbox();
  try {
    const plain = powershellRun(sandbox, resumeCommand(["--model", "claude-opus-5-5"], RESUME_ID, false));
    assert.deepEqual(plain.calls, [["--resume", RESUME_ID, "--model", "claude-opus-5-5"]], plain.stderr);
    for (const bad of ["a&calc", "$(calc)", "a;calc", "a`b", "%COMSPEC%", 'a"b']) {
      const run = powershellRun(sandbox, resumeCommand(["--model", bad], RESUME_ID, false));
      assert.deepEqual(run.calls, [["--resume", RESUME_ID]], JSON.stringify(bad));
      assert.equal(run.canary, false, JSON.stringify(bad));
    }
  } finally {
    fs.rmSync(sandbox.dir, { recursive: true, force: true });
  }
});

test("PowerShell — carryOn 문구는 큰따옴표 안의 한 칸이다(변수·부분식 확장 글자가 없다)", { skip: !onWindows }, () => {
  const sandbox = cmdSandbox();
  try {
    const run = powershellRun(sandbox, resumeCommand([], RESUME_ID, true));
    assert.equal(run.calls.length, 1, run.stderr);
    assert.equal(run.calls[0].length, 3);
    assert.deepEqual(run.calls[0].slice(0, 2), ["--resume", RESUME_ID]);
  } finally {
    fs.rmSync(sandbox.dir, { recursive: true, force: true });
  }
});
