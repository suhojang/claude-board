// @ts-nocheck — sessionCommand.test.ts 와 sessionCommand.cmd.test.ts 가 함께 쓰는 시험 자료 (node:test 로만 읽는다)
import { DEFAULT_SESSION_COMMAND, freshSessionCommand, namedCommand } from "./sessionCommand.ts";
import { sessionNameArg } from "./sessionName.ts";

// 폴더 이름에 올 수 있는 글자와 올 수 없지만 입력 쪽을 믿지 않고 넣어 보는 글자
export const hostileFolders = [
  "shop-api", "my app  v2", "내 프로젝트", "한글&calc", "a&b", "& calc", "a|b", "a^b", "ab^", "%COMSPEC%", "%PATH%",
  "!COMSPEC!", "a!b", 'a"b', 'a" & calc & "b', "a'b", "(x) a)b", "a) & (calc", "dir\\", 'dir\\"', "a,b;c=d",
  "x“;calc;“", "a>b<c", "a\r\ncalc", "--dangerously-skip-permissions", "-n", "", "   ", "con", "...", "ＡＢＣ ａｂｃ",
  "가".repeat(255), "x".repeat(300),
];

export const newSession = (folder) => {
  const fresh = freshSessionCommand(DEFAULT_SESSION_COMMAND);
  return { line: namedCommand(fresh.command, sessionNameArg(folder)), sessionId: fresh.sessionId };
};
