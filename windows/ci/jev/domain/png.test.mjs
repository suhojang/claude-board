import { test } from 'node:test';
import assert from 'node:assert/strict';
import { PngError, crop, decode, encode, readHeader } from './png.mjs';
import { makeFilteredPng, makePng } from '../testkit/helpers.mjs';

const pixel = (x, y) => [(x * 7 + y) % 256, (y * 13 + x * 3) % 256, (x * y) % 256];

test('쓰고 다시 읽으면 같은 픽셀이다', () => {
  const buf = makePng(23, 17, pixel);
  const image = decode(buf);
  assert.equal(image.width, 23);
  assert.equal(image.height, 17);
  for (const [x, y] of [[0, 0], [22, 16], [5, 9]]) {
    assert.deepEqual([...image.data.subarray((y * 23 + x) * 3, (y * 23 + x) * 3 + 3)], pixel(x, y));
  }
});

test('필터 다섯 종류를 모두 푼다', () => {
  for (const filters of [[0], [1], [2], [3], [4], [0, 1, 2, 3, 4]]) {
    const image = decode(makeFilteredPng(19, 11, pixel, filters));
    for (let y = 0; y < 11; y += 1) {
      for (let x = 0; x < 19; x += 1) {
        assert.deepEqual([...image.data.subarray((y * 19 + x) * 3, (y * 19 + x) * 3 + 3)], pixel(x, y), `filters ${filters} at ${x},${y}`);
      }
    }
  }
});

test('자르기: 사각형 안 픽셀만 남고 크기가 맞다', () => {
  const cropped = decode(crop(makePng(40, 30, pixel), { x: 10, y: 5, width: 12, height: 8 }));
  assert.equal(cropped.width, 12);
  assert.equal(cropped.height, 8);
  assert.deepEqual([...cropped.data.subarray(0, 3)], pixel(10, 5));
  assert.deepEqual([...cropped.data.subarray((7 * 12 + 11) * 3, (7 * 12 + 11) * 3 + 3)], pixel(21, 12));
});

test('자르기: 이미지 밖·잘못된 사각형은 거절한다', () => {
  const buf = makePng(40, 30);
  for (const rect of [{ x: 30, y: 0, width: 11, height: 5 }, { x: 0, y: 25, width: 5, height: 6 }, { x: -1, y: 0, width: 5, height: 5 }, { x: 0, y: 0, width: 0, height: 5 }, { x: 0.5, y: 0, width: 5, height: 5 }]) {
    assert.throws(() => crop(buf, rect), PngError, JSON.stringify(rect));
  }
});

test('RGBA 도 자른다(CDP 캡처 모양)', () => {
  const data = Buffer.alloc(8 * 6 * 4, 0x7f);
  const out = decode(crop(encode({ width: 8, height: 6, colorType: 6, data }), { x: 1, y: 1, width: 3, height: 2 }));
  assert.equal(out.channels, 4);
  assert.equal(out.data.length, 3 * 2 * 4);
});

test('PNG 가 아니거나 지원 밖이면 시끄럽게 거절한다', () => {
  assert.throws(() => readHeader(Buffer.from('not a png at all, definitely not a png at all')), PngError);
  const palette = Buffer.from(makePng(4, 4));
  palette[25] = 3; // 색 종류 = 팔레트
  assert.throws(() => decode(palette), /색 종류/);
  const interlaced = Buffer.from(makePng(4, 4));
  interlaced[28] = 1;
  assert.throws(() => decode(interlaced), /인터레이스/);
  const deep = Buffer.from(makePng(4, 4));
  deep[24] = 16;
  assert.throws(() => decode(deep), /비트 깊이/);
});

test('maxPixels 를 넘으면 풀기 전에 거절한다', () => {
  assert.throws(() => decode(makePng(40, 30), { maxPixels: 1000 }), /상한/);
});

test('잘린 파일은 거절한다', () => {
  const buf = makePng(40, 30);
  assert.throws(() => decode(buf.subarray(0, buf.length - 20)), PngError);
});
