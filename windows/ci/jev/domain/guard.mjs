// 무엇을 Jev 에 보여 줄 수 있나. 문자열·헤더 규칙이며 그림 속 내용을 읽지는 않는다.
//
// 막는 것: e2e 폴더 밖 파일, PNG 가 아닌 파일, 하네스 한도를 벗어난 크기, 회사 경로, (allowedWindowTitle 을 주면) 다른 창 제목으로 자른 그림.
// 못 막는 것: 그림 픽셀 속 내용 — 보드 창만 담겼는지, 계정 칩의 이메일·사용량 패널이 들어갔는지. 호출하는 쪽이 보드 창만 잘라 넘긴다(README).
// 글(진술·메타데이터)의 개인정보는 domain/privacy.mjs 가 막는다.
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { IMAGE_LIMITS, SOURCE_MAX_PIXELS } from '../questions.mjs';
import { PngError, readHeader } from './png.mjs';

export class GuardError extends Error {}

const COMPANY_MARKER = 'aijinet-project';

// 하네스 guard.ensure_under_e2e 는 <--repo>/e2e/ 아래만 허용한다. 우리는 그 안에서도 git 이 무시하는 하위 폴더만 허용한다.
export const ALLOWED_SUBDIRS = Object.freeze(['out', 'artifacts']);

function segments(p) {
  return String(p).replace(/\\/g, '/').split('/').filter(Boolean);
}

export function ensureNotCompany(...paths) {
  for (const p of paths) {
    if (segments(p).includes(COMPANY_MARKER)) throw new GuardError(`회사 자료는 Jev 로 보내지 않는다: ${p}`);
  }
}

function real(p, what) {
  try {
    return fs.realpathSync.native(p);
  } catch {
    throw new GuardError(`${what}을(를) 찾을 수 없다: ${p}`);
  }
}

// 경로가 <repo>/e2e/<허용 하위 폴더>/ 아래 파일이어야 한다. 심볼릭 링크는 실제 경로로 푼 뒤에 따진다.
export function ensureUnderE2e(filePath, repo, { mustExist = true } = {}) {
  ensureNotCompany(filePath, repo);
  const e2e = path.join(real(repo, '저장소 폴더'), 'e2e');
  const target = mustExist ? real(filePath, '파일') : path.join(real(path.dirname(filePath), '폴더'), path.basename(filePath));
  const rel = path.relative(e2e, target);
  const parts = rel.split(path.sep);
  if (rel === '' || path.isAbsolute(rel) || parts[0] === '..' || parts.length < 2 || !ALLOWED_SUBDIRS.includes(parts[0])) {
    throw new GuardError(`이미지는 ${path.join(e2e, `{${ALLOWED_SUBDIRS.join(',')}}`)} 아래에 있어야 한다: ${filePath}`);
  }
  return target;
}

// jevvision.domain.image.fit_size 의 가능 여부만 옮긴 것(반올림이 파이썬과 달라도 가능 여부 판정에는 영향이 없다).
export function fitSizeFeasible(width, height) {
  const { minSide, maxSide, minPixels, maxPixels } = IMAGE_LIMITS;
  let w = width;
  let h = height;
  if (Math.max(w, h) > maxSide) {
    const scale = maxSide / Math.max(w, h);
    w = Math.max(1, Math.round(width * scale));
    h = Math.max(1, Math.round(height * scale));
  }
  if (Math.min(w, h) < minSide) {
    const scale = minSide / Math.min(width, height);
    w = Math.max(minSide, Math.ceil(width * scale));
    h = Math.max(minSide, Math.ceil(height * scale));
  }
  const sidesOk = w >= minSide && w <= maxSide && h >= minSide && h <= maxSide;
  return sidesOk && w * h >= minPixels && w * h <= maxPixels ? { width: w, height: h } : null;
}

// PNG 헤더와 크기. 바이트가 PNG 가 아니면 확장자가 .png 여도 거절한다.
export function checkPng(buf) {
  let header;
  try {
    header = readHeader(buf);
  } catch (error) {
    if (error instanceof PngError) throw new GuardError(`PNG 가 아니다: ${error.message}`);
    throw error;
  }
  if (buf.length > IMAGE_LIMITS.maxFileBytes) throw new GuardError(`파일이 ${IMAGE_LIMITS.maxFileBytes} 바이트를 넘는다: ${buf.length}`);
  if (header.width * header.height > SOURCE_MAX_PIXELS) throw new GuardError(`픽셀 수가 ${SOURCE_MAX_PIXELS} 를 넘는다: ${header.width}x${header.height}`);
  if (!fitSizeFeasible(header.width, header.height)) {
    throw new GuardError(`하네스 이미지 한도 안으로 맞출 수 없는 크기다: ${header.width}x${header.height}`);
  }
  return header;
}

// 한 번에: 위치 -> 읽기 -> PNG 검사. 통과하면 {path, bytes, header}.
export function guardImage(imagePath, repo, readFile = fs.readFileSync) {
  const target = ensureUnderE2e(imagePath, repo);
  const bytes = readFile(target);
  return { path: target, bytes, header: checkPng(bytes) };
}

// ── 자른 그림의 manifest ──────────────────────────────────────────────────────
// `crop` 이 그림 옆에 `<그림>.manifest.json` 을 쓴다: {version, windowTitle, rect, sha256(자른 그림 바이트)}.
// 호출하는 쪽이 allowedWindowTitle 을 주면 그림은 그 창 제목으로 자른 것이고 바이트가 그대로여야 한다.
// 못 막는 것: 그 창 안에 그려진 내용(계정 이메일 등) — 픽셀은 읽지 않는다. 창 제목은 호출하는 쪽이 정한다.
export const MANIFEST_VERSION = 1;
export const manifestPathOf = (imagePath) => `${imagePath}.manifest.json`;
export const sha256Hex = (bytes) => crypto.createHash('sha256').update(bytes).digest('hex');

export function checkCropManifest(imagePath, bytes, allowedWindowTitle, readFile = fs.readFileSync) {
  const file = manifestPathOf(imagePath);
  let manifest;
  try {
    manifest = JSON.parse(readFile(file, 'utf8'));
  } catch (error) {
    throw new GuardError(`자른 그림의 manifest 가 없거나 읽을 수 없다(crop --window-title 로 자른 그림만 보낸다): ${path.basename(file)} (${error.code ?? 'bad json'})`);
  }
  if (manifest?.version !== MANIFEST_VERSION) throw new GuardError(`manifest 버전이 다르다: ${manifest?.version}`);
  if (manifest.sha256 !== sha256Hex(bytes)) throw new GuardError('manifest 의 sha256 이 그림과 다르다 — 자른 뒤에 그림이 바뀌었다');
  if (typeof manifest.windowTitle !== 'string' || manifest.windowTitle !== allowedWindowTitle) {
    throw new GuardError('그림을 자른 창 제목이 허용한 제목과 다르다');
  }
  return manifest;
}
