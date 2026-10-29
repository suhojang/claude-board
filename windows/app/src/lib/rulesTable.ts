// @ts-nocheck — node 전용 규칙 표 로더(시험만 import 한다). node:fs 를 쓰므로 svelte-check 대상이 아니다
// 꼴 소유자: rules-table/README.md — 같은 뜻으로 도는 Rust 쪽: windows/app/src-tauri/src/rules_table.rs
// 실행: cd windows/app && node --test src/lib/rulesTable.test.ts
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

/** 규칙 표의 줄 하나 — rules-table/README.md 의 case 꼴. */
export type RuleCase = {
  /** <family>.<rule>.<영문 slug> — 파일 안에서 유일, 한 번 쓴 id 는 다시 쓰지 않는다 */
  id: string;
  /** 돌리는 쪽이 이 이름으로 판정 함수를 고른다 */
  rule: string;
  /** 의도한 동작(한국어) — 사람이 읽는 근거이고 출력으로 만들지 않는다 */
  why: string;
  /** 근거(PR·주인 문장). 선택 — 없으면 null */
  source: string | null;
  input: unknown;
  expect: unknown;
};

/** rule 하나의 판정 함수 — 통과면 null, 실패면 까닭 문자열. 던져도 실패다. */
export type RuleFn = (c: RuleCase) => string | null;

/** `only == "mac"` 줄은 프론트 돌리는 쪽이 건너뛴다(mac 쪽이 돌릴 줄) */
const SKIPPED_ONLY = "mac";

/** 저장소 맨 위 rules-table/ 폴더. 이 파일(windows/app/src/lib)의 다섯 층 위다. */
const RULES_TABLE_DIR = path.resolve(
  fileURLToPath(import.meta.url),
  "..",
  "..",
  "..",
  "..",
  "..",
  "rules-table",
);

/** 줄의 필수 문자열 키를 꺼낸다. 비지 않은 문자열만 받는다. */
function needString(row, key) {
  const value = row[key];
  if (typeof value === "string" && value !== "") return value;
  throw new Error(`${key} 는 비지 않은 문자열이어야 한다`);
}

/** 표 본문을 꼴대로 확인한다. family 값과 건너뛴 뒤의 줄들을 돌려준다. */
export function parseTable(text) {
  const table = JSON.parse(text);
  if (table === null || typeof table !== "object" || Array.isArray(table)) {
    throw new Error("규칙 표 최상위는 객체여야 한다");
  }
  if (typeof table.family !== "string" || table.family === "") {
    throw new Error("family 는 비지 않은 문자열이어야 한다");
  }
  if (typeof table.schema !== "number" || table.schema !== 1) {
    throw new Error(`schema 는 숫자 1 이어야 한다(지금 ${JSON.stringify(table.schema)})`);
  }
  if (!Array.isArray(table.cases)) {
    throw new Error("cases 는 배열이어야 한다");
  }

  // 줄에서 그 밖에 쓸 수 있는 키는 이 일곱뿐이다. 모르는 키는 꼴 오류다.
  const CASE_KEYS = ["id", "rule", "why", "source", "only", "input", "expect"];
  const seen = new Set();
  const cases = [];
  for (const raw of table.cases) {
    if (raw === null || typeof raw !== "object" || Array.isArray(raw)) {
      throw new Error("case 는 객체여야 한다");
    }
    for (const key of Object.keys(raw)) {
      if (!CASE_KEYS.includes(key)) {
        throw new Error(`모르는 키 '${key}' 가 있는 줄이 있다`);
      }
    }
    const id = needString(raw, "id");
    const rule = needString(raw, "rule");
    const why = needString(raw, "why");
    let source = null; // 선택
    if (raw.source !== undefined) {
      if (typeof raw.source !== "string") throw new Error("source 는 문자열이어야 한다");
      source = raw.source;
    }
    const only = raw.only;
    if (only !== undefined && only !== "mac" && only !== "windows") {
      throw new Error('only 는 "mac" 또는 "windows" 이어야 한다');
    }
    if (!("input" in raw)) throw new Error("input 이 없는 줄이 있다");
    if (!("expect" in raw)) throw new Error("expect 이 없는 줄이 있다");
    if (seen.has(id)) throw new Error(`id 가 겹친다: ${id}`);
    seen.add(id);
    if (only === SKIPPED_ONLY) continue; // mac 쪽만 돌릴 줄 — 프론트 돌리는 쪽은 건너뛴다
    cases.push({ id, rule, why, source, input: raw.input, expect: raw.expect });
  }
  return { family: table.family, cases };
}

/** rules-table/<family>.json 을 읽어 줄로 돌려준다. 파일이 없거나 꼴이 틀리거나
 * JSON 의 family 값이 파일 이름과 다르면 던져 멈춘다(setup 실수를 조용히 못 넘어가게). */
export function loadTable(family) {
  const filePath = path.join(RULES_TABLE_DIR, `${family}.json`);
  let raw;
  try {
    raw = fs.readFileSync(filePath, "utf8");
  } catch (error) {
    throw new Error(`규칙 표 파일을 읽지 못했다: ${filePath} (${error.message})`);
  }
  const { family: fileFamily, cases } = parseTable(raw);
  if (fileFamily !== family) {
    throw new Error(`규칙 표 ${filePath} 의 family 값은 '${fileFamily}' 인데 파일 이름은 '${family}' 다`);
  }
  return cases;
}

/** 줄 전부를 돌린다. 모르는 rule 과 판정 실패를 모아 한 번에 던지고, 돈 줄이 0 이면 실패다. */
export function runCases(cases, rules) {
  if (cases.length === 0) {
    throw new Error("돌린 줄이 0 개다");
  }
  const failures = [];
  for (const one of cases) {
    const ruleFn = rules[one.rule];
    if (ruleFn === undefined) {
      failures.push(`${one.id}: 모르는 rule '${one.rule}'`);
      continue;
    }
    let reason = null;
    try {
      reason = ruleFn(one);
    } catch (error) {
      failures.push(`${one.id}: ${error.message}`);
      continue;
    }
    if (reason != null) failures.push(`${one.id}: ${reason}`);
  }
  if (failures.length > 0) throw new Error(failures.join("\n"));
}

/** loadTable 로 줄을 읽고 전부 돌린다. 실패(모은 실패 id·모르는 rule·0 줄)면 그대로 던져 멈춘다. */
export function runTable(family, rules) {
  runCases(loadTable(family), rules);
}