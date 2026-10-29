// jevvision.cli 대역. 어댑터가 하위 프로세스로 부르는 것과 같은 인자를 받고, 환경변수 JEV_STUB 이 고른 종료코드·줄을 낸다.
// JEV_STUB_ARGV_LOG 가 있으면 받은 인자(JSON)와 환경에 TYPESAFE_API_KEY 가 있는지 여부(값은 아님)를 한 줄로 덧붙인다.
import fs from 'node:fs';

const args = process.argv.slice(2);
const scenario = process.env.JEV_STUB ?? 'pass';
if (process.env.JEV_STUB_ARGV_LOG) {
  fs.appendFileSync(
    process.env.JEV_STUB_ARGV_LOG,
    `${JSON.stringify({ args, keyInEnv: 'TYPESAFE_API_KEY' in process.env, cwd: process.cwd() })}\n`,
  );
}
const checks = [];
for (let i = 0; i < args.length; i += 1) if (args[i] === '--check') checks.push({ name: args[i + 1], expect: args[i + 3] });
const emit = (...lines) => lines.forEach((l) => console.log(l));
const checkLine = (status, c) => `CHECK|${status}|${c.name}|expect=${c.expect}|noul=${status === 'PASS' ? '0.97' : '0.04'}`;

const NOKEY = 'ERROR|6|PreconditionError|Windows 자격 증명 관리자 조회는 아직 구현되지 않았다(gamerig-win 몫). 환경변수 TYPESAFE_API_KEY 를 쓴다';

let rc = 0;
switch (scenario) {
  case 'pass': emit(...checks.map((c) => checkLine('PASS', c)), 'JEV|calls=1|cache_hits=0|input_tokens=3000|model=jev-1.14.0'); break;
  case 'fail': emit(...checks.map((c, i) => checkLine(i === 0 ? 'FAIL' : 'PASS', c))); rc = 1; break;
  case 'unsure': emit(...checks.map((c, i) => checkLine(i === 0 ? 'UNSURE' : 'PASS', c))); rc = 3; break;
  case 'ask': emit('ASK|check|--no-jev: Jev 를 쓰지 않는다'); rc = 3; break;
  case 'nokey': emit(NOKEY); rc = 6; break;
  case 'precondition': emit('ERROR|6|PreconditionError|venv 가 없다'); rc = 6; break;
  case 'error7': emit('ERROR|7|JevError|jev 500 (request -): '); rc = 7; break;
  case 'budget4': emit('ERROR|4|BudgetExceeded|Jev 호출 예산 6 회를 다 썼다'); rc = 4; break;
  case 'guard5': emit('ERROR|5|GuardViolation|이미지는 e2e 아래에 있어야 한다'); rc = 5; break;
  case 'usage2': emit('ERROR|2|UsageError|기대값은 yes 또는 no'); rc = 2; break;
  case 'crash': console.error('Traceback (most recent call last):\n  File "x", line 1\nImportError: PIL'); rc = 1; break;
  case 'silent0': rc = 0; break;
  case 'pass-but-fail-line': emit(...checks.map((c) => checkLine('FAIL', c))); rc = 0; break;
  case 'long-token':
    emit('Traceback junk line that must be dropped: sk-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa');
    emit('ERROR|7|JevError|jev 401 Bearer aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa');
    rc = 7;
    break;
  case 'screen': emit(`SCREEN|${process.env.JEV_STUB_SCREEN ?? 'board-expanded'}|conf=0.95|popup=0.01|system_dialog=0.00|loading=0.00`); break;
  case 'selftest-pass': emit('SELFTEST|PASS|box=0.2,0.2,0.4,0.4|conf=0.95'); break;
  case 'hang': await new Promise((resolve) => setTimeout(resolve, 60_000)); break;
  default: console.error(`알 수 없는 시나리오: ${scenario}`); rc = 99;
}
process.exitCode = rc;
