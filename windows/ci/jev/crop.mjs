// 스크린샷을 보드 창 사각형으로 잘라 e2e/out 에 쓴다. Node 내장만 쓴다(domain/png.mjs).
// 전체 화면 캡처를 Jev 에 보내지 않도록, 호출하는 쪽은 보드 창의 사각형(x,y,너비,높이)으로 먼저 자른다.
// 입력은 어디에 있어도 되지만(로컬 파일을 읽을 뿐이다) 출력은 <repo>/e2e/{out,artifacts}/ 아래만 쓴다.
import fs from 'node:fs';
import path from 'node:path';
import { GuardError, MANIFEST_VERSION, ensureNotCompany, ensureUnderE2e, manifestPathOf, sha256Hex, ALLOWED_SUBDIRS } from './domain/guard.mjs';
import { PngError, crop, readHeader } from './domain/png.mjs';
import { SOURCE_MAX_PIXELS } from './questions.mjs';

export function parseRect(text) {
  const parts = String(text).split(',').map((p) => p.trim());
  if (parts.length !== 4 || !parts.every((p) => /^\d+$/.test(p))) throw new GuardError(`사각형은 x,y,너비,높이 정수 넷이다: ${text}`);
  const [x, y, width, height] = parts.map(Number);
  return { x, y, width, height };
}

// 쓰기 전에 출력 위치를 글자로 먼저 따지고(폴더를 만들기 전에) 만든 뒤 실제 경로로 다시 따진다.
// windowTitle: 자르는 창의 제목(호출하는 쪽이 정한다). 그림 옆 manifest 에 남고 check/screen 의 --allowed-window-title 이 대조한다.
export function cropFile({ input, rect, output, repo, windowTitle }) {
  ensureNotCompany(input, output);
  const e2e = path.join(path.resolve(repo), 'e2e');
  const lexical = path.relative(e2e, path.resolve(output)).split(path.sep);
  if (lexical[0] === '..' || path.isAbsolute(path.relative(e2e, path.resolve(output))) || lexical.length < 2 || !ALLOWED_SUBDIRS.includes(lexical[0])) {
    throw new GuardError(`자른 그림은 ${path.join(e2e, `{${ALLOWED_SUBDIRS.join(',')}}`)} 아래에만 쓴다: ${output}`);
  }
  if (!output.toLowerCase().endsWith('.png')) throw new GuardError(`출력은 .png 여야 한다: ${output}`);
  let bytes;
  try {
    bytes = fs.readFileSync(input);
  } catch (error) {
    throw new GuardError(`입력을 읽을 수 없다: ${input} (${error.code})`);
  }
  let cropped;
  try {
    readHeader(bytes);
    cropped = crop(bytes, rect, { maxPixels: SOURCE_MAX_PIXELS });
  } catch (error) {
    if (error instanceof PngError) throw new GuardError(`자를 수 없다: ${error.message}`);
    throw error;
  }
  fs.mkdirSync(path.dirname(path.resolve(output)), { recursive: true });
  const target = ensureUnderE2e(output, repo, { mustExist: false }); // 폴더를 만든 뒤 실제 경로(링크 해석 후)로 다시 따진다
  fs.writeFileSync(target, cropped);
  const manifest = { version: MANIFEST_VERSION, windowTitle: windowTitle ?? null, rect, sha256: sha256Hex(cropped) };
  fs.writeFileSync(manifestPathOf(target), JSON.stringify(manifest), 'utf8');
  return { path: target, width: rect.width, height: rect.height };
}
