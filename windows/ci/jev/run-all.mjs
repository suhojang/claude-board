// `node --test windows/ci/jev/` 의 진입점. Node 24 는 폴더 인자를 폴더 안 파일 모음이 아니라 하나의 실행 대상으로 읽으므로,
// package.json 의 main 이 이 파일을 가리키게 하고 여기서 폴더 안의 *.test.mjs 를 전부 불러온다.
// 파일 하나만 돌릴 때는 `node --test windows/ci/jev/cache.test.mjs` 처럼 직접 지정한다.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const root = path.dirname(fileURLToPath(import.meta.url));
const files = [];
(function walk(dir) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const file = path.join(dir, entry.name);
    if (entry.isDirectory()) walk(file);
    else if (entry.name.endsWith('.test.mjs')) files.push(file);
  }
})(root);
if (files.length === 0) throw new Error('시험 파일이 없다');
for (const file of files.sort()) await import(pathToFileURL(file).href);
