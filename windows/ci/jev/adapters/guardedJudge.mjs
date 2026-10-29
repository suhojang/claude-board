// Judge 를 감싸 이미지와 글을 먼저 검사하는 데코레이터. 통과하지 못한 것은 안쪽(캐시·하위 프로세스)에 닿지 않는다.
// 위반은 GuardError 를 던진다 — 호출한 쪽이 GUARD 로 보고한다. 판정으로 삼키지 않는다.
//
// 검사하고 쓰는 사이에 파일이 바뀌지 못하게 이미지는 한 번만 읽는다. 그 바이트를 검사하고, 같은 바이트를
// 새로 만든 비공개 폴더(<repo>/e2e/out/jev-in-*)의 임시 사본으로 써서 안쪽에 그 사본 경로를 준다. 끝나면 지운다.
// 검사하는 것: 이미지 위치·PNG(domain/guard.mjs), 진술·선택지·이름의 개인정보(domain/privacy.mjs),
//   allowedWindowTitle 을 주면 자른 그림의 manifest(창 제목·바이트 해시).
// 못 막는 것: 그림 픽셀 속 내용. 호출하는 쪽이 보드 창 사각형으로 crop 해서 넘긴다(README).
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { GuardError, checkCropManifest, guardImage } from '../domain/guard.mjs';
import { privacyBlockAll } from '../domain/privacy.mjs';
import { normalizeItem } from '../port.mjs';

export function currentUserName(env = process.env) {
  try {
    return env.USERNAME || env.USER || os.userInfo().username || '';
  } catch {
    return env.USERNAME || env.USER || '';
  }
}

export function guardedJudge(inner, { repo, allowedWindowTitle, userName = currentUserName(), readFile = fs.readFileSync }) {
  // fields: 보내는 글 {자리 이름: 글}. run(안쪽에 줄 이미지 경로)
  async function withGuardedImage(imagePath, fields, run) {
    const { path: source, bytes } = guardImage(imagePath, repo, readFile);
    const texts = { ...fields };
    if (allowedWindowTitle !== undefined) {
      const manifest = checkCropManifest(source, bytes, allowedWindowTitle);
      texts.windowTitle = manifest.windowTitle;
      texts.allowedWindowTitle = allowedWindowTitle;
    }
    const blocked = privacyBlockAll(texts, userName);
    if (blocked) throw new GuardError(`privacy guard: ${blocked}`);
    const scratch = path.join(fs.realpathSync.native(repo), 'e2e', 'out');
    fs.mkdirSync(scratch, { recursive: true });
    const folder = fs.mkdtempSync(path.join(scratch, 'jev-in-'));
    try {
      const copy = path.join(folder, 'image.png');
      fs.writeFileSync(copy, bytes, { flag: 'wx' });
      return await run(copy);
    } finally {
      fs.rmSync(folder, { recursive: true, force: true });
    }
  }

  const itemFields = (items) =>
    Object.fromEntries(items.flatMap((item, i) => [[`name[${i}]`, item.name], [`statement[${i}]`, item.statement], [`expect[${i}]`, item.expect]]));

  return {
    check: (imagePath, statement, expect) => {
      const item = normalizeItem(statement, expect);
      return withGuardedImage(imagePath, itemFields([item]), (copy) => inner.check(copy, statement, expect));
    },
    checkAll: (imagePath, items) => {
      const normalized = items.map((item, i) => normalizeItem(item, 'yes', i));
      return withGuardedImage(imagePath, itemFields(normalized), (copy) => inner.checkAll(copy, items));
    },
    screen: (imagePath, choices, expect) =>
      withGuardedImage(imagePath, { choices: choices.join(','), expect: expect ?? '' }, (copy) => inner.screen(copy, choices, expect)),
    selftest: inner.selftest ? () => inner.selftest() : undefined,
  };
}
