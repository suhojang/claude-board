//! D1 캡처의 자식 프로세스(node) 스크립트 모음 — 작업 부하마다 하나. 모두 끝에 `DONE\r\n` 을 쓴다.

/// 부하 이름 → 스크립트. 알 수 없는 이름이면 None.
pub fn script(load: &str) -> Option<&'static str> {
    Some(match load {
        "ascii" => ASCII,
        "ko" => KO,
        "ko-odd" => KO_ODD,
        "tui" => TUI,
        "ink" => INK,
        _ => return None,
    })
}

/// 줄 번호가 든 `line <n> <x 96개>` 를 50줄씩 묶어 빠르게 쏟는다(1000줄마다 잠깐 쉼).
const ASCII: &str = r#"
let n = 0; const x = 'x'.repeat(96);
function burst() {
  let s = '';
  for (let i = 0; i < 50 && n < 6000; i++) { n++; s += 'line ' + n + ' ' + x + '\r\n'; }
  process.stdout.write(s);
  if (n >= 6000) { process.stdout.write('DONE\r\n'); setTimeout(() => process.exit(0), 400); return; }
  setTimeout(burst, n % 1000 === 0 ? 60 : 1);
}
burst();
"#;

/// 한글: `line <n> ` + 가 48개(96칸). 번호 자릿수가 바뀌면 시작 칸의 홀짝이 바뀌어 넓은 글자가 홀수·짝수 칸에 번갈아 놓인다.
const KO: &str = r#"
let n = 0; const x = '가'.repeat(48);
function burst() {
  let s = '';
  for (let i = 0; i < 50 && n < 6000; i++) { n++; s += 'line ' + n + ' ' + x + '\r\n'; }
  process.stdout.write(s);
  if (n >= 6000) { process.stdout.write('DONE\r\n'); setTimeout(() => process.exit(0), 400); return; }
  setTimeout(burst, n % 1000 === 0 ? 60 : 1);
}
burst();
"#;

/// 한글 홀수 칸: `line <n> x` + 가 48개 — KO 와 홀짝이 반대라 두 부하가 서로 보완한다.
const KO_ODD: &str = r#"
let n = 0; const x = '가'.repeat(48);
function burst() {
  let s = '';
  for (let i = 0; i < 50 && n < 6000; i++) { n++; s += 'line ' + n + ' x' + x + '\r\n'; }
  process.stdout.write(s);
  if (n >= 6000) { process.stdout.write('DONE\r\n'); setTimeout(() => process.exit(0), 400); return; }
  setTimeout(burst, n % 1000 === 0 ? 60 : 1);
}
burst();
"#;

/// 대체 화면 TUI: `ESC[?1049h`, 커서 숨김, 18x50 상자를 절대 위치(CUP)로 한 번 그리고(창 크기 이벤트에도 다시 그림),
/// 100 ms 마다 상태 칸 한 곳만 갱신한다. 35 프레임(약 3.5 s) 뒤 `DONE`.
const TUI: &str = r#"
const E = '\x1b';
function box() {
  let s = E + '[2J';
  for (let r = 0; r < 18; r++) s += E + '[' + (3 + r) + ';5H|' + (' row ' + r + ' ').padEnd(48, '.') + '|';
  process.stdout.write(s);
}
process.stdout.write(E + '[?1049h' + E + '[?25l');
box();
process.stdout.on('resize', box);
let k = 0;
const t = setInterval(() => {
  k++;
  process.stdout.write(E + '[23;5H' + ('status frame ' + k).padEnd(30));
  if (k >= 35) { clearInterval(t); process.stdout.write(E + '[25;1HDONE\r\n'); setTimeout(() => process.exit(0), 400); }
}, 100);
"#;

/// Claude Code 식 다시 그리기: 주 화면에 기록 20줄을 찍고, 아래 6줄 블록을 20 Hz 로 `ESC[6A ESC[J` 로 지우고 다시 쓴다. 60 프레임 뒤 `DONE`.
const INK: &str = r#"
const E = '\x1b';
let s = '';
for (let i = 1; i <= 20; i++) s += 'history ' + i + ' ' + 'h'.repeat(30) + '\r\n';
process.stdout.write(s);
let k = 0;
const t = setInterval(() => {
  k++;
  let o = k > 1 ? E + '[6A' + E + '[J' : '';
  o += 'block title frame ' + k + '\r\n';
  for (let i = 1; i <= 4; i++) o += '  item ' + i + ' ' + String(k * i).padStart(5, '0') + ' ' + 'b'.repeat(20) + '\r\n';
  o += '> prompt ' + k + '\r\n';
  process.stdout.write(o);
  if (k >= 60) { clearInterval(t); process.stdout.write('DONE\r\n'); setTimeout(() => process.exit(0), 400); }
}, 50);
"#;
