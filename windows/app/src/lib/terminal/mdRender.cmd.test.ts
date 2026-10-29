// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 실행: cd windows/app && node --test src/lib/terminal/mdRender.cmd.test.ts   (Windows 에서만 돈다)
// 보드는 mdRenderCommand 가 만든 줄을 셸 탭에 그대로 쳐 넣는다. 공백·한글·& ( ) ; , ' 가 든 폴더가 cmd.exe·PowerShell 에서도 argv 한 칸으로 가는지 잰다.
// sessionCommand.cmd.test.ts 와 같은 방식 — PATH 앞에 둔 claude.cmd 가 받은 인자를 node 로 JSON 에 남긴다. 순수 규칙 시험은 mdRender.test.ts 에 있다.
import { test } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { mdRenderCommand } from "./mdRender.ts";
import { freshSessionCommand, namedCommand } from "./sessionCommand.ts";
import { sessionNameArg } from "./sessionName.ts";

const onWindows = process.platform === "win32";

const folders = [
  String.raw`C:\Users\홍 길동\.claude\notifier\plugins\md-render`,
  String.raw`C:\Users\a b\.claude\notifier\plugins\md-render`,
  String.raw`D:\한글 폴더\a&b (x);y,z'w\plugins\md-render`,
  String.raw`C:\Users\me\.claude\notifier\plugins\md-render`,
];

function sandbox() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "claude-board-md-"));
  fs.writeFileSync(path.join(dir, "dump.cjs"), "require('fs').appendFileSync(process.env.ARGV_OUT, JSON.stringify(process.argv.slice(2)) + '\\n');");
  fs.writeFileSync(path.join(dir, "claude.cmd"), `@"${process.execPath}" "%~dp0dump.cjs" %*\r\n`);
  const argvOut = path.join(dir, "argv.jsonl");
  const env = { ...process.env, PATH: dir + path.delimiter + process.env.PATH, ARGV_OUT: argvOut };
  const calls = () => (fs.existsSync(argvOut) ? fs.readFileSync(argvOut, "utf8").trim().split("\n").map((row) => JSON.parse(row)) : []);
  const reset = () => fs.rmSync(argvOut, { force: true });
  return {
    dir,
    inCmd(line) {
      reset();
      const result = spawnSync(process.env.ComSpec || "cmd.exe", ["/d", "/s", "/c", `"${line}"`], { cwd: dir, env, windowsVerbatimArguments: true, encoding: "utf8" });
      return { calls: calls(), status: result.status, stderr: result.stderr };
    },
    inPowerShell(program, line) {
      reset();
      const result = spawnSync(program, ["-NoProfile", "-NonInteractive", ...(program === "powershell.exe" ? ["-ExecutionPolicy", "Bypass"] : []), "-Command", line], { cwd: dir, env, encoding: "utf8" });
      return { calls: calls(), status: result.status, stderr: result.stderr };
    },
  };
}

const hasPwsh = onWindows && spawnSync("pwsh.exe", ["-NoProfile", "-Command", "1"], { encoding: "utf8" }).status === 0;
const SESSION = "8f3c2a10-1111-4222-8333-444455556666";

function newLine(dir, off) {
  const fresh = { command: `claude --session-id ${SESSION}` };
  return mdRenderCommand(namedCommand(fresh.command, sessionNameArg("my app")), dir, off);
}

for (const [shell, run] of [
  ["cmd.exe", (box, line) => box.inCmd(line)],
  ["Windows PowerShell 5.1", (box, line) => box.inPowerShell("powershell.exe", line)],
  ["pwsh", (box, line) => box.inPowerShell("pwsh.exe", line)],
]) {
  const skip = !onWindows || (shell === "pwsh" && !hasPwsh);
  test(`${shell} — --plugin-dir 값은 공백·한글·메타문자가 있어도 argv 한 칸이고 claude 는 한 번만 불린다`, { skip }, () => {
    const box = sandbox();
    try {
      for (const dir of folders) {
        const run1 = run(box, newLine(dir, false));
        assert.deepEqual(run1.calls, [["--plugin-dir", dir, "--name", "my app", "--session-id", SESSION]], `${dir} ${run1.stderr}`);
      }
    } finally {
      fs.rmSync(box.dir, { recursive: true, force: true });
    }
  });

  test(`${shell} — 끄면 --plugin-dir 이 없다`, { skip }, () => {
    const box = sandbox();
    try {
      assert.deepEqual(run(box, newLine(folders[0], true)).calls, [["--name", "my app", "--session-id", SESSION]]);
    } finally {
      fs.rmSync(box.dir, { recursive: true, force: true });
    }
  });

  test(`${shell} — 이미 든 옛 --plugin-dir 은 걷히고 하나만 간다`, { skip }, () => {
    const box = sandbox();
    try {
      const old = String.raw`C:\Users\me\AppData\Local\Programs\Claude Board\plugins\md-render`;
      const line = mdRenderCommand(`claude --plugin-dir "${old}" --resume abc --plugin-dir "${old}"`, folders[0], false);
      assert.deepEqual(run(box, line).calls, [["--plugin-dir", folders[0], "--resume", "abc"]]);
    } finally {
      fs.rmSync(box.dir, { recursive: true, force: true });
    }
  });
}

test("cmd.exe — 이어 열기 줄(freshSessionCommand 없이)에도 같은 규칙", { skip: !onWindows }, () => {
  const box = sandbox();
  try {
    const line = mdRenderCommand(namedCommand(freshSessionCommand("claude --resume abc").command, sessionNameArg("proj")), folders[2], false);
    assert.deepEqual(box.inCmd(line).calls, [["--plugin-dir", folders[2], "--name", "proj", "--resume", "abc"]]);
  } finally {
    fs.rmSync(box.dir, { recursive: true, force: true });
  }
});
