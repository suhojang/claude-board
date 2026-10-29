// 판정 해시 캐시: (판정자 id·version + 진술 + 기대값 + 이미지 sha256) -> 판정 JSON 한 파일. 같은 화면·같은 질문은 다시 묻지 않는다.
// 위치는 windows/e2e/out/jev-cache/ (git 이 무시한다). 하네스 jevvision 의 DiskCache 와 별개의 바깥 층이다.
// UNVERIFIED 는 저장하지 않는다 — 키가 없어서 못 본 것이 키가 생긴 뒤에도 못 본 것으로 남지 않게.
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { STATUS, assertVerdict, normalizeItem } from './port.mjs';

export const CACHE_VERSION = 2;

export const sha256 = (bytes) => crypto.createHash('sha256').update(bytes).digest('hex');

export class VerdictCache {
  constructor(directory) {
    this.directory = directory;
  }

  static key(parts) {
    return sha256(JSON.stringify([CACHE_VERSION, ...parts]));
  }

  #file(key) {
    if (!/^[0-9a-f]{64}$/.test(key)) throw new TypeError(`캐시 키가 sha256 hex 가 아니다: ${key}`);
    return path.join(this.directory, `v-${key}.json`);
  }

  get(key) {
    const file = this.#file(key);
    if (!fs.existsSync(file)) return null;
    try {
      return assertVerdict(JSON.parse(fs.readFileSync(file, 'utf8')));
    } catch {
      throw new Error(`판정 캐시가 깨졌다: ${file}`); // 삼키지 않는다. 지우고 다시 돌린다
    }
  }

  // 저장했으면 true.
  put(key, verdict) {
    assertVerdict(verdict);
    if (verdict.status === STATUS.UNVERIFIED) return false;
    const file = this.#file(key); // 키 검사가 먼저다(폴더를 만들기 전에)
    fs.mkdirSync(this.directory, { recursive: true });
    const temporary = `${file}.${process.pid}.tmp`;
    fs.writeFileSync(temporary, JSON.stringify(verdict), 'utf8');
    fs.renameSync(temporary, file);
    return true;
  }
}

// Judge 를 감싸 캐시 적중 때는 안쪽을 부르지 않는 데코레이터. 적중한 판정에는 cached: true 가 붙는다.
// 키에는 판정자의 id 와 version 이 들어간다 — 다른 판정자(가짜·실제)나 바뀐 판정자가 옛 판정을 물려받지 않게.
// 판정자가 id·version 을 안 밝히면 id 는 클래스 이름, version 은 0 이다.
export function judgeIdentity(judge) {
  return [String(judge?.id ?? judge?.constructor?.name ?? 'judge'), Number(judge?.version ?? 0)];
}

export function cachedJudge(inner, cache, readFile = fs.readFileSync) {
  const identity = judgeIdentity(inner);
  const imageSha = (imagePath) => sha256(readFile(imagePath));
  async function through(key, call) {
    const hit = cache.get(key);
    if (hit) return { ...hit, cached: true };
    const verdict = await call();
    cache.put(key, verdict);
    return verdict;
  }
  return {
    check(imagePath, statement, expect = 'yes') {
      const item = normalizeItem(statement, expect);
      return through(VerdictCache.key([...identity, 'check', item.statement, item.expect, imageSha(imagePath)]), () => inner.check(imagePath, item));
    },
    // 적중하지 못한 질문만 한 요청으로 묶어 안쪽에 묻는다.
    async checkAll(imagePath, items) {
      const sha = imageSha(imagePath);
      const normalized = items.map((item, i) => normalizeItem(item, 'yes', i));
      const keys = normalized.map((item) => VerdictCache.key([...identity, 'check', item.statement, item.expect, sha]));
      const verdicts = keys.map((key) => {
        const hit = cache.get(key);
        return hit ? { ...hit, cached: true } : null;
      });
      const missing = verdicts.flatMap((v, i) => (v ? [] : [i]));
      if (missing.length > 0) {
        const fresh = await inner.checkAll(imagePath, missing.map((i) => normalized[i]));
        missing.forEach((at, n) => {
          verdicts[at] = fresh[n];
          cache.put(keys[at], fresh[n]);
        });
      }
      return verdicts;
    },
    screen(imagePath, choices, expect) {
      return through(
        VerdictCache.key([...identity, 'screen', [...choices], expect ?? null, imageSha(imagePath)]),
        () => inner.screen(imagePath, choices, expect),
      );
    },
    selftest: inner.selftest ? () => inner.selftest() : undefined,
  };
}
