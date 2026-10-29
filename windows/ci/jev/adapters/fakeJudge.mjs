// 시험용 Judge. 미리 정해 둔 답을 돌려주고 받은 호출을 기록한다. 답이 안 정해진 질문은 UNVERIFIED 다(기본이 PASS 가 아니다).
import { normalizeItem, unverified } from '../port.mjs';

// script.check: 판정 | 판정 배열(호출 순서) | (imagePath, item) => 판정
// script.screen: 같은 모양. script.selftest: 판정
export class FakeJudge {
  id = 'fake';
  version = 1;

  constructor(script = {}) {
    this.script = script;
    this.calls = [];
    this.counters = { check: 0, screen: 0 };
  }

  #answer(kind, args) {
    const rule = this.script[kind];
    const index = this.counters[kind]++;
    if (typeof rule === 'function') return rule(...args);
    if (Array.isArray(rule)) return rule[index] ?? unverified('fake: no scripted answer');
    return rule ?? unverified('fake: no scripted answer');
  }

  async check(imagePath, statement, expect = 'yes') {
    const item = normalizeItem(statement, expect);
    this.calls.push({ verb: 'check', imagePath, item });
    return this.#answer('check', [imagePath, item]);
  }

  async checkAll(imagePath, items) {
    const normalized = items.map((item, i) => normalizeItem(item, 'yes', i));
    this.calls.push({ verb: 'checkAll', imagePath, items: normalized });
    return normalized.map((item) => this.#answer('check', [imagePath, item]));
  }

  async screen(imagePath, choices, expect) {
    this.calls.push({ verb: 'screen', imagePath, choices: [...choices], expect });
    return this.#answer('screen', [imagePath, choices, expect]);
  }

  async selftest() {
    this.calls.push({ verb: 'selftest' });
    return this.script.selftest ?? unverified('fake: no scripted answer');
  }
}
