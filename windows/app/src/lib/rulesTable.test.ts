// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 공유 규칙 표 로더 시험 — 같은 뜻의 Rust 쪽 시험: windows/app/src-tauri/src/rules_table.rs tests
// 실행: cd windows/app && node --test src/lib/rulesTable.test.ts
import { test } from "node:test";
import assert from "node:assert/strict";
import { loadTable, parseTable, runCases, runTable } from "./rulesTable.ts";

/** 줄 세 개의 정상 표. */
const GOOD = `{
  "family": "probe",
  "schema": 1,
  "cases": [
    {"id": "probe.probe.alpha", "rule": "probe", "why": "첫 줄", "input": {"a": 1}, "expect": {"a": 1}},
    {"id": "probe.probe.beta", "rule": "probe", "why": "둘째 줄", "source": "PR #0", "input": {}, "expect": {}},
    {"id": "probe.probe.gamma", "rule": "probe", "why": "셋째 줄", "input": {}, "expect": {}}
  ]
}`;

const cases = (text) => parseTable(text).cases;

test("id 를 다시 쓴 줄이 있으면 실패한다", () => {
  const text = `{
    "family": "probe",
    "schema": 1,
    "cases": [
      {"id": "probe.probe.same", "rule": "probe", "why": "먼저 쓴 줄", "input": {}, "expect": {}},
      {"id": "probe.probe.same", "rule": "probe", "why": "id 를 다시 쓴 줄", "input": {}, "expect": {}}
    ]
  }`;
  assert.throws(() => parseTable(text), /id 가 겹친다: probe\.probe\.same/);
});

test("why 가 없는 줄은 꼴 오류이다", () => {
  const text = `{
    "family": "probe",
    "schema": 1,
    "cases": [
      {"id": "probe.probe.nowhy", "rule": "probe", "input": {}, "expect": {}}
    ]
  }`;
  assert.throws(() => parseTable(text), /why 는 비지 않은 문자열이어야 한다/);
});

test("schema 1 이 아닌 표는 꼴 오류이다", () => {
  const text = `{"family": "probe", "schema": 2, "cases": []}`;
  assert.throws(() => parseTable(text), /schema 는 숫자 1 이어야 한다/);
});

test("정상 표는 줄 전부를 순서대로 돌리고 값이 온전히 실려 간다", () => {
  const ran = [];
  const table = cases(GOOD);
  assert.equal(table.length, 3);
  assert.deepEqual(table[0].input, { a: 1 }, "input 은 JSON 그대로 실려 가야 한다");
  assert.deepEqual(table[0].expect, { a: 1 }, "expect 도 그대로다");
  assert.equal(table[0].why, "첫 줄");
  assert.equal(table[1].source, "PR #0");
  assert.equal(table[2].source, null, "source 는 없으면 없는 것이다");
  runCases(table, { probe: (c) => (ran.push(c.id), null) });
  assert.deepEqual(ran, ["probe.probe.alpha", "probe.probe.beta", "probe.probe.gamma"]);
});

test("only mac 인 줄은 건너뛰고 only 값이 이상하면 꼴 오류이다", () => {
  const text = `{
    "family": "probe",
    "schema": 1,
    "cases": [
      {"id": "probe.probe.here", "rule": "probe", "why": "둘 다 돌리는 줄", "input": {}, "expect": {}},
      {"id": "probe.probe.macthat", "only": "mac", "rule": "probe", "why": "mac 쪽만 돌리는 줄", "input": {}, "expect": {}}
    ]
  }`;
  const table = cases(text);
  assert.deepEqual(
    table.map((c) => c.id),
    ["probe.probe.here"],
  );
  const badOnly = `{
    "family": "probe",
    "schema": 1,
    "cases": [
      {"id": "probe.probe.stray", "only": "both", "rule": "probe", "why": "값이 틀린 줄", "input": {}, "expect": {}}
    ]
  }`;
  assert.throws(() => parseTable(badOnly), /only 는 "mac" 또는 "windows" 이어야 한다/);
});

test("모르는 rule 은 그 줄을 실패로 모은다", () => {
  const table = cases(GOOD);
  runCases(table, { probe: () => null });
  assert.throws(
    () => runCases(table, { other: () => null }),
    /probe\.probe\.alpha: 모르는 rule 'probe'/,
  );
});

test("돈 줄이 0 이면 실패한다", () => {
  const table = cases(`{"family": "probe", "schema": 1, "cases": []}`);
  assert.throws(() => runCases(table, { probe: () => null }), /0 개다/);
});

test("실패 id 를 한 번에 모아 보인다", () => {
  const table = cases(GOOD);
  assert.throws(
    () => runCases(table, { probe: () => "항상 실패한다" }),
    (error) => {
      assert.match(error.message, /probe\.probe\.alpha: 항상 실패한다/);
      assert.match(error.message, /probe\.probe\.beta: 항상 실패한다/);
      assert.match(error.message, /probe\.probe\.gamma: 항상 실패한다/);
      // 세 줄의 실패가 한 Error 안에 함께 모인다(첫 실패에서 끊지 않는다)
      assert.equal((error.message.match(/항상 실패한다/g) || []).length, 3);
      return true;
    },
  );
});

test("표 파일이 없으면 조용히 넘어가지 않는다", () => {
  // probe.json 은 실제 표로 만들지 않을 이름이라 파일이 없는 것이 보장된다
  assert.throws(() => loadTable("probe"), /규칙 표 파일을 읽지 못했다/);
  assert.throws(() => runTable("probe", { probe: () => null }), /규칙 표 파일을 읽지 못했다/);
});