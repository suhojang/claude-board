// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 실행: cd windows/app && node --test src/lib/board/windowMenu.test.ts
import { test } from "node:test";
import assert from "node:assert/strict";
import { snapHint, snapIconName, snapMenuLabel, windowMenuItems } from "./windowMenu.ts";

const LEFT = "처음 크기로 왼쪽에 붙이기";
const RIGHT = "처음 크기로 오른쪽에 붙이기";

test("붙이기 항목 글자는 boardSide 만 따른다", () => {
  assert.equal(snapMenuLabel("left"), LEFT);
  assert.equal(snapMenuLabel("right"), RIGHT);
});

test("boardSide 가 없거나 알 수 없으면 오른쪽(mac 은 == left 만 왼쪽)", () => {
  for (const v of [undefined, null, "", "Left", "center"]) assert.equal(snapMenuLabel(v), RIGHT, String(v));
});

test("단추 안내와 그림도 같은 쪽을 따른다", () => {
  assert.equal(snapHint("left"), "처음 크기로 되돌려 화면 왼쪽에 붙이기");
  assert.equal(snapHint("right", "Ctrl+Alt+B"), "처음 크기로 되돌려 화면 오른쪽에 붙이기 (Ctrl+Alt+B)");
  assert.equal(snapHint("right", null), "처음 크기로 되돌려 화면 오른쪽에 붙이기");
  assert.equal(snapIconName("left"), "panel-left");
  assert.equal(snapIconName("right"), "snap-right");
});

test("상태 열여섯 조합: 글자는 side 만, 체크는 expanded·pinned 만 따르고 mini 는 아무것도 안 바꾼다", () => {
  const bools = [false, true];
  let n = 0;
  for (const side of ["left", "right"])
    for (const mini of bools)
      for (const expanded of bools)
        for (const pinned of bools) {
          const items = windowMenuItems({ side, mini, expanded, pinned });
          const tag = JSON.stringify({ side, mini, expanded, pinned });
          assert.deepEqual(
            items,
            [
              { id: "snap", label: side === "left" ? LEFT : RIGHT },
              { id: "terminal", label: "터미널 펼치기", checked: expanded },
              { id: "pin", label: "항상 위에 고정", checked: pinned },
            ],
            tag,
          );
          n++;
        }
  assert.equal(n, 16);
});
