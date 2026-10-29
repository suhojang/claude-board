// 시험 공용 도구. 임시 저장소 폴더(<tmp>/repo/e2e/out/...)와 합성 PNG.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import zlib from 'node:zlib';
import { SIGNATURE, encode } from '../domain/png.mjs';

export function tempRepo() {
  const root = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), 'jev-test-')));
  const repo = path.join(root, 'repo');
  fs.mkdirSync(path.join(repo, 'e2e', 'out'), { recursive: true });
  return { root, repo, out: path.join(repo, 'e2e', 'out'), cleanup: () => fs.rmSync(root, { recursive: true, force: true }) };
}

// pixel(x, y) -> [r,g,b] 로 칠한 RGB PNG.
export function makePng(width, height, pixel = (x, y) => [x % 256, y % 256, (x + y) % 256]) {
  const data = Buffer.alloc(width * height * 3);
  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) data.set(pixel(x, y), (y * width + x) * 3);
  }
  return encode({ width, height, colorType: 2, data });
}

// 필터 종류를 줄마다 바꿔 쓰는 PNG(해독기 시험용). filters[y] = 0..4, RGB 8비트.
export function makeFilteredPng(width, height, pixel, filters) {
  const channels = 3;
  const stride = width * channels;
  const rows = [];
  for (let y = 0; y < height; y += 1) {
    rows.push(Buffer.from(Array.from({ length: stride }, (_, i) => pixel(Math.floor(i / channels), y)[i % channels])));
  }
  const raw = Buffer.alloc(height * (stride + 1));
  for (let y = 0; y < height; y += 1) {
    const filter = filters[y % filters.length];
    raw[y * (stride + 1)] = filter;
    for (let i = 0; i < stride; i += 1) {
      const x = rows[y][i];
      const a = i >= channels ? rows[y][i - channels] : 0;
      const b = y > 0 ? rows[y - 1][i] : 0;
      const c = y > 0 && i >= channels ? rows[y - 1][i - channels] : 0;
      const p = a + b - c;
      const pa = Math.abs(p - a);
      const pb = Math.abs(p - b);
      const pc = Math.abs(p - c);
      const paeth = pa <= pb && pa <= pc ? a : pb <= pc ? b : c;
      const predictor = [0, a, b, (a + b) >> 1, paeth][filter];
      raw[y * (stride + 1) + 1 + i] = (x - predictor) & 0xff;
    }
  }
  const chunk = (type, data) => {
    const out = Buffer.alloc(12 + data.length);
    out.writeUInt32BE(data.length, 0);
    out.write(type, 4, 'latin1');
    data.copy(out, 8);
    out.writeUInt32BE(zlib.crc32(out.subarray(4, 8 + data.length)) >>> 0, 8 + data.length);
    return out;
  };
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8;
  ihdr[9] = 2;
  return Buffer.concat([SIGNATURE, chunk('IHDR', ihdr), chunk('IDAT', zlib.deflateSync(raw)), chunk('IEND', Buffer.alloc(0))]);
}

export function writePng(dir, name, width = 320, height = 300) {
  const file = path.join(dir, name);
  fs.writeFileSync(file, makePng(width, height));
  return file;
}
