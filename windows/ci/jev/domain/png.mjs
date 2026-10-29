// PNG 읽기·자르기·쓰기. Node 내장(zlib)만 쓴다. 8비트 비인터레이스 PNG(회색·회색+알파·RGB·RGBA)만 푼다.
// CDP 캐처·PrintWindow 결과가 이 모양이다. 팔레트·16비트·인터레이스는 시끄럽게 거절한다(조용히 틀린 그림을 내지 않는다).
import zlib from 'node:zlib';

export class PngError extends Error {}

export const SIGNATURE = Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]);
const CHANNELS = { 0: 1, 2: 3, 4: 2, 6: 4 };

export function readHeader(buf) {
  if (buf.length < 33 || !buf.subarray(0, 8).equals(SIGNATURE)) throw new PngError('PNG 서명이 아니다');
  if (buf.readUInt32BE(8) !== 13 || buf.toString('latin1', 12, 16) !== 'IHDR') throw new PngError('첫 청크가 IHDR 이 아니다');
  const header = {
    width: buf.readUInt32BE(16),
    height: buf.readUInt32BE(20),
    bitDepth: buf[24],
    colorType: buf[25],
    interlace: buf[28],
  };
  if (header.width < 1 || header.height < 1) throw new PngError(`크기가 잘못됐다: ${header.width}x${header.height}`);
  return header;
}

function* chunks(buf) {
  let at = 8;
  while (at + 12 <= buf.length) {
    const length = buf.readUInt32BE(at);
    const type = buf.toString('latin1', at + 4, at + 8);
    if (at + 12 + length > buf.length) throw new PngError('청크가 잘렸다');
    yield { type, data: buf.subarray(at + 8, at + 8 + length) };
    at += 12 + length;
    if (type === 'IEND') return;
  }
}

function paeth(a, b, c) {
  const p = a + b - c;
  const pa = Math.abs(p - a);
  const pb = Math.abs(p - b);
  const pc = Math.abs(p - c);
  if (pa <= pb && pa <= pc) return a;
  return pb <= pc ? b : c;
}

// maxPixels 를 주면 풀기 전에 그보다 큰 이미지를 거절한다(압축 폭탄 방지). 안 주면 상한이 없으니 신뢰 못 할 입력에는 꼭 준다.
export function decode(buf, { maxPixels = Infinity } = {}) {
  const header = readHeader(buf);
  const channels = CHANNELS[header.colorType];
  if (!channels) throw new PngError(`지원하지 않는 색 종류: ${header.colorType} (팔레트는 RGB 로 다시 저장한다)`);
  if (header.bitDepth !== 8) throw new PngError(`지원하지 않는 비트 깊이: ${header.bitDepth}`);
  if (header.interlace !== 0) throw new PngError('인터레이스 PNG 는 지원하지 않는다');
  if (header.width * header.height > maxPixels) throw new PngError(`픽셀 수가 상한을 넘는다: ${header.width}x${header.height}`);
  const idat = [];
  for (const chunk of chunks(buf)) if (chunk.type === 'IDAT') idat.push(chunk.data);
  if (idat.length === 0) throw new PngError('IDAT 가 없다');
  const stride = header.width * channels;
  let raw;
  try {
    raw = zlib.inflateSync(Buffer.concat(idat));
  } catch (error) {
    throw new PngError(`압축을 풀 수 없다: ${error.code ?? error.message}`);
  }
  if (raw.length !== header.height * (stride + 1)) throw new PngError('픽셀 자료 길이가 맞지 않는다');
  const data = Buffer.alloc(header.height * stride);
  const bpp = channels;
  for (let y = 0; y < header.height; y += 1) {
    const filter = raw[y * (stride + 1)];
    const src = y * (stride + 1) + 1;
    const row = y * stride;
    const prev = row - stride;
    for (let i = 0; i < stride; i += 1) {
      const x = raw[src + i];
      const a = i >= bpp ? data[row + i - bpp] : 0;
      const b = y > 0 ? data[prev + i] : 0;
      const c = y > 0 && i >= bpp ? data[prev + i - bpp] : 0;
      let value;
      switch (filter) {
        case 0: value = x; break;
        case 1: value = x + a; break;
        case 2: value = x + b; break;
        case 3: value = x + ((a + b) >> 1); break;
        case 4: value = x + paeth(a, b, c); break;
        default: throw new PngError(`알 수 없는 필터: ${filter}`);
      }
      data[row + i] = value & 0xff;
    }
  }
  return { width: header.width, height: header.height, colorType: header.colorType, channels, data };
}

function chunk(type, data) {
  const out = Buffer.alloc(12 + data.length);
  out.writeUInt32BE(data.length, 0);
  out.write(type, 4, 'latin1');
  data.copy(out, 8);
  out.writeUInt32BE(zlib.crc32(out.subarray(4, 8 + data.length)) >>> 0, 8 + data.length);
  return out;
}

// 필터 없음(0)으로 쓴다. 보조 청크는 쓰지 않는다(화면 밖 메타데이터가 따라가지 않게).
export function encode({ width, height, colorType, data }) {
  const channels = CHANNELS[colorType];
  if (!channels) throw new PngError(`지원하지 않는 색 종류: ${colorType}`);
  const stride = width * channels;
  if (data.length !== height * stride) throw new PngError('픽셀 자료 길이가 맞지 않는다');
  const raw = Buffer.alloc(height * (stride + 1));
  for (let y = 0; y < height; y += 1) {
    data.copy(raw, y * (stride + 1) + 1, y * stride, (y + 1) * stride);
  }
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8;
  ihdr[9] = colorType;
  return Buffer.concat([SIGNATURE, chunk('IHDR', ihdr), chunk('IDAT', zlib.deflateSync(raw)), chunk('IEND', Buffer.alloc(0))]);
}

// 사각형 밖이면 거절한다(자르다 말고 줄이면 설정 실수가 가려진다).
export function crop(buf, rect, options) {
  const { x, y, width, height } = rect;
  for (const v of [x, y, width, height]) {
    if (!Number.isInteger(v)) throw new PngError('사각형은 정수 x,y,너비,높이여야 한다');
  }
  if (x < 0 || y < 0 || width < 1 || height < 1) throw new PngError('사각형이 잘못됐다');
  const image = decode(buf, options);
  if (x + width > image.width || y + height > image.height) {
    throw new PngError(`사각형이 이미지(${image.width}x${image.height}) 밖이다`);
  }
  const out = Buffer.alloc(width * height * image.channels);
  for (let row = 0; row < height; row += 1) {
    const from = ((y + row) * image.width + x) * image.channels;
    image.data.copy(out, row * width * image.channels, from, from + width * image.channels);
  }
  return encode({ width, height, colorType: image.colorType, data: out });
}
