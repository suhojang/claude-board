// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 실행: cd windows/app && node --test src/lib/terminal/repaintScanner.test.ts
import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import { RepaintScanner } from "./repaintScanner.ts";

const E = "\x1b";
const TARGET = { cols: 80, rows: 24 };
const NOTICE = `${E}[8;24;80t`;
const bytes = (s) => Uint8Array.from(Buffer.from(s, "latin1"));
const filler = (n) => "x".repeat(n);

/** 프레임들을 이어서 먹인다. 매 단계 safe 가 줄지 않고 cut 을 넘지 않는지 함께 본다 */
function run(scanner, frames, startSeq = 0) {
  let seq = startSeq;
  let last = { safe: startSeq, cut: null };
  let prevSafe = startSeq;
  for (const f of frames) {
    const b = typeof f === "string" ? bytes(f) : f;
    last = scanner.feed(seq, b);
    seq += b.length;
    assert.ok(last.safe >= prevSafe, "safe 는 줄지 않는다");
    assert.ok(last.safe <= seq, "safe 는 받은 끝을 넘지 않는다");
    if (last.cut !== null) assert.equal(last.safe, last.cut, "cut 이 정해지면 safe 는 cut");
    prevSafe = last.safe;
  }
  return last;
}

const make = (o = {}) => new RepaintScanner({ target: TARGET, cut: "A", alt: false, anchorSeq: 200, ...o });
const LEAD = filler(200); // 기준(200) 앞의 앱 출력

// 측정된 머리말 꼴 다섯 (ESC[H 바로 앞까지가 머리말)
const FORMS = [
  { name: "?25l 알림 LF", pre: `${E}[?25l${NOTICE}\n`, alt: false },
  { name: "?25l 알림 CUP LF", pre: `${E}[?25l${NOTICE}${E}[24;1H\n`, alt: false },
  { name: "알림만(대체 화면)", pre: NOTICE, alt: true },
  { name: "?25l CUP LF(알림 없음)", pre: `${E}[?25l${E}[24;1H\n`, alt: false },
  { name: "?25l 만", pre: `${E}[?25l`, alt: false },
];

test("머리말 꼴마다 ESC[H 바로 앞에서 자른다", () => {
  for (const f of FORMS) {
    const data = LEAD + f.pre + `${E}[H` + "repaint";
    const cut = LEAD.length + f.pre.length;
    const r = run(make({ alt: f.alt }), [data]);
    assert.equal(r.cut, cut, f.name);
    assert.equal(r.safe, cut, f.name);
  }
});

test("머리말을 모든 바이트 위치에서 두 프레임으로 갈라도 같은 자리에서 자른다", () => {
  const forms = [...FORMS, { name: "맨 ESC[H(대체 화면)", pre: "", alt: true }];
  for (const f of forms) {
    const data = LEAD + f.pre + `${E}[H` + "repaint";
    const cut = LEAD.length + f.pre.length;
    for (let off = 1; off < data.length; off++) {
      const s = make({ alt: f.alt });
      const r = run(s, [data.slice(0, off), data.slice(off)]);
      assert.equal(r.cut, cut, `${f.name} @${off}`);
    }
  }
});

test("꼬리 ESC 는 1바이트, ESC[ 는 2바이트 붙잡고 ESC[? 는 붙잡지 않는다", () => {
  const a = make();
  assert.equal(a.feed(0, bytes(LEAD + "ab" + E)).safe, LEAD.length + 2);
  const b = make();
  assert.equal(b.feed(0, bytes(LEAD + "ab" + E + "[")).safe, LEAD.length + 2);
  const c = make();
  assert.equal(c.feed(0, bytes(LEAD + "ab" + E + "[?")).safe, LEAD.length + 5);
  const d = make();
  assert.equal(d.feed(0, bytes(LEAD + "ab" + "[")).safe, LEAD.length + 3);
  // 붙잡은 뒤 다른 바이트가 오면 풀린다
  assert.equal(a.feed(LEAD.length + 3, bytes("z")).safe, LEAD.length + 4);
});

test("붙잡은 ESC 가 한동안 안 이어지면 flushIdle 이 풀어 준다", () => {
  const s = make();
  s.feed(0, bytes(LEAD + E));
  assert.equal(s.safe, LEAD.length);
  assert.equal(s.flushIdle(), null);
  assert.equal(s.safe, LEAD.length + 1);
});

test("기준-64 앞 바이트의 머리말은 무시한다", () => {
  const old = `${E}[?25l${NOTICE}\n${E}[H`;
  const s = new RepaintScanner({ target: TARGET, cut: "A", alt: false, anchorSeq: 500 });
  // 머리말이 seq 0..: 기준-64 = 436 앞
  const r1 = run(s, [old + filler(600)]);
  assert.equal(r1.cut, null);
  assert.equal(r1.safe, old.length + 600);
  // 그 뒤에 진짜가 오면 잡는다
  const real = `${E}[?25l${NOTICE}\n`;
  const r2 = s.feed(old.length + 600, bytes(real + `${E}[H`));
  assert.equal(r2.cut, old.length + 600 + real.length);
});

test("머리말이 기준-64 에 걸쳐 잘리면 후보가 아니다", () => {
  // 알림 시작이 436 앞이고 ESC[H 는 그 뒤: 되돌아 읽기는 436 부터라 알림이 온전하지 않다
  const pre = `${NOTICE}`;
  const startOfNotice = 436 - 3;
  const data = filler(startOfNotice) + pre + `${E}[H`;
  const s = new RepaintScanner({ target: TARGET, cut: "A", alt: false, anchorSeq: 500 });
  assert.equal(run(s, [data]).cut, null);
});

test("다른 크기의 알림은 버리고 계속 찾는다", () => {
  const wrong = `${E}[?25l${E}[8;30;100t\n${E}[H`;
  const right = `${E}[?25l${NOTICE}\n`;
  const data = LEAD + wrong + filler(50) + right + `${E}[H` + "z";
  const r = run(make(), [data]);
  assert.equal(r.cut, LEAD.length + wrong.length + 50 + right.length);
});

test("다른 크기의 알림을 잘못 받지 않는다(끝까지 없으면 cut 없음)", () => {
  const wrong = `${E}[?25l${E}[8;24;81t\n${E}[H`;
  const r = run(make(), [LEAD + wrong + filler(100)]);
  assert.equal(r.cut, null);
});

test("맨 ESC[H 는 대체 화면에서만 받는다", () => {
  const data = LEAD + "abc" + `${E}[H` + "def";
  assert.equal(run(make({ alt: false }), [data]).cut, null);
  assert.equal(run(make({ alt: true }), [data]).cut, LEAD.length + 3);
});

test("맨 ESC[H 는 기준에서 4 KiB 안에서만 받는다", () => {
  const near = LEAD + filler(4000) + `${E}[H`;
  assert.equal(run(make({ alt: true }), [near]).cut, LEAD.length + 4000);
  const far = LEAD + filler(4100) + `${E}[H`;
  assert.equal(run(make({ alt: true }), [far]).cut, null);
});

test("앱의 CUP(LF ESC[2;5H)은 후보가 아니다", () => {
  const data = LEAD + "line\n" + `${E}[2;5H` + "text\r\n" + `${E}[10;1H` + "more";
  assert.equal(run(make({ alt: false }), [data]).cut, null);
  assert.equal(run(make({ alt: true }), [data]).cut, null);
});

test("컷 B: ESC[?25h 바로 뒤에서 자른다", () => {
  const pre = `${E}[?25l${NOTICE}\n${E}[H`;
  const body = "paint" + `${E}[?25h` + "after";
  const data = LEAD + pre + body;
  const cut = LEAD.length + pre.length + "paint".length + 6;
  const r = run(make({ cut: "B" }), [data]);
  assert.equal(r.cut, cut);
  assert.equal(r.safe, cut);
});

test("컷 B: 머리말과 ESC[?25h 를 모든 위치에서 갈라도 같은 자리", () => {
  const pre = `${E}[?25l${NOTICE}\n${E}[H`;
  const data = LEAD + pre + "paint" + `${E}[?25h` + "after";
  const cut = LEAD.length + pre.length + "paint".length + 6;
  for (let off = 1; off < data.length; off++) {
    const r = run(make({ cut: "B" }), [data.slice(0, off), data.slice(off)]);
    assert.equal(r.cut, cut, `@${off}`);
  }
});

test("컷 B: ESC[?25h 의 앞부분은 최대 5바이트 붙잡고 아니면 푼다", () => {
  const pre = `${E}[?25l${NOTICE}\n${E}[H`;
  for (let m = 1; m <= 5; m++) {
    const s = make({ cut: "B" });
    const tail = `${E}[?25h`.slice(0, m);
    const r = s.feed(0, bytes(LEAD + pre + "paint" + tail));
    assert.equal(r.cut, null);
    assert.equal(r.safe, LEAD.length + pre.length + 5, `m=${m}`);
  }
  const s = make({ cut: "B" });
  assert.equal(s.feed(0, bytes(LEAD + pre + "paint" + E + "[?2x")).safe, LEAD.length + pre.length + 10);
});

test("컷 B: 붙잡은 앞부분이 한동안 안 이어지면 flushIdle 이 지금 끝에서 자른다", () => {
  const pre = `${E}[?25l${NOTICE}\n${E}[H`;
  const s = make({ cut: "B" });
  const end = LEAD.length + pre.length + 5 + 3;
  s.feed(0, bytes(LEAD + pre + "paint" + `${E}[?`));
  assert.equal(s.flushIdle(), end - 0 + 0 - 0);
});

test("컷 B: 예산 안에 ESC[?25h 가 있으면 그것을 쓴다", () => {
  const small = { cols: 10, rows: 2 }; // 예산 2*(40+16)+256 = 368
  const pre = `${E}[H`;
  const a = LEAD.length;
  const data = LEAD + `${E}[?25l${E}[8;2;10t\n` + pre + "ab\ncd" + filler(300) + `${E}[?25h` + "z";
  const s = make({ target: small, cut: "B" });
  const r = run(s, [data]);
  const pos = data.indexOf(`${E}[?25h`) + 6;
  assert.equal(r.cut, pos);
  assert.ok(pos - data.indexOf(pre, a) <= 368);
});

test("컷 B: 예산을 넘기면 ESC[?25h 가 있어도 그 뒤 첫 LF 다음에서 자른다(한 프레임)", () => {
  const small = { cols: 10, rows: 2 };
  const pre = `${E}[?25l${E}[8;2;10t\n${E}[H`;
  const data = LEAD + pre + "ab\ncd" + filler(500) + `${E}[?25h` + "z";
  const lf = data.indexOf("\n", LEAD.length + pre.length);
  const r = run(make({ target: small, cut: "B" }), [data]);
  assert.equal(r.cut, lf + 1);
});

test("컷 B: 예산을 넘기면 이미 내보낸 바이트 뒤의 다음 LF 다음에서 자른다(작은 프레임)", () => {
  const small = { cols: 10, rows: 2 };
  const pre = `${E}[?25l${E}[8;2;10t\n${E}[H`;
  const first = LEAD + pre + filler(400); // 예산 368 초과, LF 없음
  const s = make({ target: small, cut: "B" });
  const r1 = s.feed(0, bytes(first));
  assert.equal(r1.cut, null); // LF 를 기다린다
  assert.equal(r1.safe, first.length);
  const r2 = s.feed(first.length, bytes("tail\nnext"));
  assert.equal(r2.cut, first.length + 5);
  assert.equal(r2.safe, first.length + 5);
});

test("lineFallback: 다음 LF 바로 뒤에서 자른다", () => {
  const s = make({ anchorSeq: 0 });
  s.feed(0, bytes("abc\nline"));
  s.lineFallback();
  const r = s.feed(8, bytes("end\nrest"));
  assert.equal(r.cut, 12);
  assert.equal(r.safe, 12);
});

test("lineFallback: CR 과 LF 사이에서는 자르지 않는다", () => {
  const s = make({ anchorSeq: 0 });
  s.feed(0, bytes("ab\r"));
  s.lineFallback();
  assert.equal(s.flushIdle(), null); // CR 에서 멈춤
  const r = s.feed(3, bytes("\nxx"));
  assert.equal(r.cut, 4);
  const t = make({ anchorSeq: 0 });
  t.feed(0, bytes("ab"));
  t.lineFallback();
  assert.equal(t.flushIdle(), 2); // 줄 중간이어도 바로 자른다
});

test("flushIdle(force): CR 바로 뒤여도 자른다(스트림이 CR 에서 끝났다)", () => {
  const s = make({ anchorSeq: 0 });
  s.feed(0, bytes("ab\r"));
  s.lineFallback();
  assert.equal(s.flushIdle(), null);
  assert.equal(s.flushIdle(true), 3);
});

test("lineFallback: 이미 내보낸 LF 는 쓰지 않는다", () => {
  const s = make({ anchorSeq: 0 });
  s.feed(0, bytes("a\nb"));
  s.lineFallback();
  assert.equal(s.feed(3, bytes("c")).cut, null);
  assert.equal(s.feed(4, bytes("\nd")).cut, 5);
});

test("lineFallback: 머리말을 찾은 뒤에는 무시하고, ESC 앞부분이 붙잡혀 있으면 푼다", () => {
  const s = make({ anchorSeq: 0 });
  s.feed(0, bytes(filler(10) + E));
  s.lineFallback();
  const r = s.feed(11, bytes("[2Jxx\nyy"));
  assert.equal(r.cut, 11 + 6);
  // cut 이 이미 정해졌으면 아무 일도 없다
  const t = make({ anchorSeq: 0 });
  const done = t.feed(0, bytes(LEAD + `${E}[?25l${NOTICE}\n${E}[H`));
  t.lineFallback();
  assert.equal(t.feed(done.safe, bytes("q")).cut, done.cut);
});

// ---- 진짜 캡처(scratchpad) ----
const CAPTURES = "C:/Users/skvud/AppData/Local/Temp/claude/C--Users-skvud-harness-workspace-my-project-claude-board/65b580e4-5624-499c-9545-e2483adf0444/scratchpad/d1/out";

/** replay.mjs 168-181 의 게이트 찾기(PRE)와 같은 규칙으로 A 를 구한다 */
function gateA(text, r) {
  const PRE = /(?:\x1b\[\?25l)?(\x1b\[8;\d+;\d+t)?(?:\x1b\[\d+;1H)?\n?\x1b\[H/g;
  PRE.lastIndex = Math.max(0, r.bytesReadBeforeCall - 64);
  let mm;
  while ((mm = PRE.exec(text))) if (mm[0].length > 3) return mm.index + mm[0].length - 3;
  return null;
}

for (const name of ["d1-cap-flags0-120x30to120x20-r250", "d1-cap-flags0-120x30to120x20-r300"]) {
  test(`진짜 캡처 ${name}: A 컷이 게이트 찾기와 같다`, { skip: !fs.existsSync(`${CAPTURES}/${name}.bin`) }, () => {
    const info = JSON.parse(fs.readFileSync(`${CAPTURES}/${name}.json`, "utf8"));
    const bin = fs.readFileSync(`${CAPTURES}/${name}.bin`);
    const r = info.resize;
    const expected = gateA(bin.toString("latin1"), r);
    assert.ok(expected !== null && expected > r.bytesReadBeforeCall - 64, "게이트가 머리말을 찾는다");
    const target = { cols: r.toCols, rows: r.toRows };
    const lower = r.bytesReadBeforeCall - 64;
    // 원래 프레임 경계와, 1..9 바이트 / 1..64 KiB 로 다시 자른 경계
    const orig = info.chunks.map((c) => c[2]);
    const rng = (seed) => () => ((seed = (seed * 1664525 + 1013904223) >>> 0) / 2 ** 32);
    const rechunk = (lo, hi, seed) => {
      const next = rng(seed);
      const cum = [];
      for (let p = 0; p < bin.length; ) cum.push((p = Math.min(bin.length, p + lo + Math.floor(next() * (hi - lo + 1)))));
      return cum;
    };
    for (const cum of [orig, rechunk(1, 9, 11), rechunk(1, 65536, 7)]) {
      const s = new RepaintScanner({ target, cut: "A", alt: false, anchorSeq: r.bytesReadBeforeCall });
      let pos = 0;
      let got = null;
      for (const c of cum) {
        if (c > lower) {
          const from = Math.max(pos, lower);
          const res = s.feed(from, new Uint8Array(bin.subarray(from, c)));
          if (res.cut !== null) {
            got = res.cut;
            break;
          }
        }
        pos = c;
      }
      assert.equal(got, expected);
    }
  });
}

test("minSeq 아래의 ESC[H 는 후보가 아니다(앞 단계의 머리말 건너뛰기)", () => {
  const first = `${E}[?25l
${E}[H`; // 앞 단계의 머리말, cut 은 ESC[H 앞
  const second = `${E}[?25l${E}[24;1H
${E}[H`;
  const data = LEAD + first + "mid" + second + "tail";
  const firstCut = LEAD.length + first.length - 3;
  const secondCut = LEAD.length + first.length + 3 + second.length - 3;
  const plain = run(make({ anchorSeq: firstCut }), [data], 0);
  assert.equal(plain.cut, firstCut, "minSeq 없이는 앞 단계의 ESC[H 에 걸린다");
  const s = make({ anchorSeq: firstCut, minSeq: firstCut + 3 });
  assert.equal(run(s, [data], 0).cut, secondCut);
});
