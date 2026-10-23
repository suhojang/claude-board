// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 실행: cd windows/app && node --test src/lib/terminal/tabLogic.test.ts   (Node 22.18+/24 는 .ts 를 바로 읽는다)
import { test } from "node:test";
import assert from "node:assert/strict";
import {
  badgeNumber,
  folderNameOf,
  homeFromGroups,
  insertIndexFor,
  mostUrgent,
  needsCloseConfirm,
  nthTab,
  pickAfterClose,
  renamedTo,
  shownTabOf,
  tabsInWorkspace,
  workspaceHintOf,
  workspaceKeyOf,
  workspaceLabelOf,
  workspacesOf,
  wrapStep,
} from "./tabLogic.ts";

const HOME = "C:\\Users\\u";
const tab = (id, workspace) => ({ id, workspace });
const ids = (list) => list.map((t) => t.id);

// ── 경로 키·이름 ─────────────────────────────────────────────

test("경로 키 — 대소문자·슬래시 방향·끝 \\ 가 달라도 같은 폴더", () => {
  const k = workspaceKeyOf("C:\\Work\\Proj", HOME);
  assert.equal(workspaceKeyOf("c:/work/proj", HOME), k);
  assert.equal(workspaceKeyOf("C:\\Work\\Proj\\", HOME), k);
  assert.equal(workspaceKeyOf("C:\\Work\\Proj\\\\", HOME), k);
  assert.notEqual(workspaceKeyOf("C:\\Work\\Proj2", HOME), k);
});

test("홈은 어떻게 적어도 ~ — 문자 ~ · 전체 경로 · 슬래시 · 대소문자", () => {
  for (const p of ["~", "C:\\Users\\u", "c:/users/U/", "C:\\USERS\\U\\"]) assert.equal(workspaceKeyOf(p, HOME), "~", p);
  // 홈 아래 폴더는 홈이 아니다
  assert.equal(workspaceKeyOf("C:\\Users\\u\\proj", HOME), "c:\\users\\u\\proj");
});

test("홈을 모르면(null) 전체 경로는 홈이 아니고 ~ 글자만 홈", () => {
  assert.equal(workspaceKeyOf("C:\\Users\\u", null), "c:\\users\\u");
  assert.equal(workspaceKeyOf("~", null), "~");
});

test("홈 알아내기 — 그룹의 줄인 경로(~, ~\\x)에서 전체 경로를 거꾸로 푼다", () => {
  assert.equal(homeFromGroups([]), null);
  assert.equal(homeFromGroups([{ id: "D:\\other", path: "D:\\other" }]), null);
  assert.equal(homeFromGroups([{ id: "C:\\Users\\u", path: "~" }]), "C:\\Users\\u");
  assert.equal(homeFromGroups([{ id: "C:\\Users\\u\\proj\\api", path: "~\\proj\\api" }]), "C:\\Users\\u");
});

test("폴더 이름 — 마지막 폴더, 홈은 ~, 끝 \\ 와 / 를 무시, 드라이브 루트는 그대로", () => {
  assert.equal(folderNameOf("C:\\Work\\proj", HOME), "proj");
  assert.equal(folderNameOf("C:/Work/proj/", HOME), "proj");
  assert.equal(folderNameOf("~", HOME), "~");
  assert.equal(folderNameOf("C:\\Users\\u", HOME), "~");
  assert.equal(folderNameOf("C:\\", HOME), "C:");
  assert.equal(folderNameOf("한글 폴더", HOME), "한글 폴더");
});

test("경로 줄 이름 — 이름이 겹칠 때만 부모 폴더를 붙인다", () => {
  const among = ["C:\\a\\api", "C:\\b\\api", "C:\\a\\web", "~"];
  assert.equal(workspaceLabelOf("C:\\a\\api", among, HOME), "a/api");
  assert.equal(workspaceLabelOf("C:\\b\\api", among, HOME), "b/api");
  assert.equal(workspaceLabelOf("C:\\a\\web", among, HOME), "web");
  assert.equal(workspaceLabelOf("~", among, HOME), "~");
});

test("경로 줄 이름 — 겹침은 폴더 이름의 정확 일치로 센다(대소문자가 다르면 겹치지 않는다)", () => {
  assert.equal(workspaceLabelOf("C:\\a\\api", ["C:\\a\\api"], HOME), "api");
  assert.equal(workspaceLabelOf("C:\\a\\Api", ["C:\\a\\Api", "C:\\b\\api"], HOME), "Api");
});

test("경로 줄 이름 — 셋이 겹치면 셋 모두 부모가 붙는다, 부모가 없으면 빈 부모", () => {
  const among = ["C:\\a\\x", "C:\\b\\x", "C:\\c\\x"];
  assert.deepEqual(among.map((w) => workspaceLabelOf(w, among, HOME)), ["a/x", "b/x", "c/x"]);
  assert.equal(workspaceLabelOf("x", ["x", "y\\x"], HOME), "/x");
});

test("경로 줄 이름 — 폴더와 부모가 둘 다 같으면 조부모를 붙여 가른다", () => {
  const among = ["C:\\p\\a\\x", "C:\\q\\a\\x", "C:\\r\\b\\x"];
  assert.deepEqual(among.map((w) => workspaceLabelOf(w, among, HOME)), ["p/a/x", "q/a/x", "b/x"]);
});

test("경로 줄 이름 — 여러 마디를 거슬러 올라가도 모든 이름이 서로 다르다", () => {
  const among = ["C:\\1\\m\\n\\x", "C:\\2\\m\\n\\x", "C:\\1\\o\\n\\x"];
  const labels = among.map((w) => workspaceLabelOf(w, among, HOME));
  assert.deepEqual(labels, ["1/m/n/x", "2/m/n/x", "o/n/x"]);
  assert.equal(new Set(labels).size, labels.length);
});

test("경로 줄 이름 — 드라이브 루트 아래도 드라이브 글자까지 거슬러 올라간다", () => {
  const among = ["C:\\work\\app", "D:\\work\\app"];
  assert.deepEqual(among.map((w) => workspaceLabelOf(w, among, HOME)), ["C:/work/app", "D:/work/app"]);
  const roots = ["C:\\a\\x", "D:\\a\\x", "C:\\x"];
  assert.deepEqual(roots.map((w) => workspaceLabelOf(w, roots, HOME)), ["C:/a/x", "D:/a/x", "C:/x"]);
});

test("경로 줄 이름 — UNC 경로는 서버 이름까지 거슬러 올라간다(앞머리 \\\\ 는 마디로 세지 않는다)", () => {
  const among = ["\\\\srv1\\share\\proj", "\\\\srv2\\share\\proj"];
  assert.deepEqual(among.map((w) => workspaceLabelOf(w, among, HOME)), ["srv1/share/proj", "srv2/share/proj"]);
});

test("경로 줄 이름 — 끝 구분자와 / \\ 섞인 구분자가 이름을 바꾸지 않는다", () => {
  const among = ["C:/p/a/x/", "C:\\q\\a\\x\\\\", "C:\\r/b\\x"];
  assert.deepEqual(among.map((w) => workspaceLabelOf(w, among, HOME)), ["p/a/x", "q/a/x", "b/x"]);
});

test("경로 줄 이름 — 같은 경로(대소문자·구분자 차이뿐)는 한 항목이라 같은 이름을 받고, 다른 경로와는 갈린다", () => {
  const among = ["C:\\p\\x", "c:/P/x/", "C:\\q\\x"];
  assert.deepEqual(among.map((w) => workspaceLabelOf(w, among, HOME)), ["p/x", "P/x", "q/x"]);
  const same = ["C:\\p\\x", "c:/p/x"];
  assert.deepEqual(same.map((w) => workspaceLabelOf(w, same, HOME)), ["x", "x"]);
});

test("경로 줄 이름 — 부모 이름의 대소문자만 다른 두 경로도 서로 다르게 보이도록 더 거슬러 올라간다", () => {
  const among = ["C:\\a\\Z\\x", "C:\\b\\z\\x"];
  assert.deepEqual(among.map((w) => workspaceLabelOf(w, among, HOME)), ["a/Z/x", "b/z/x"]);
});

test("경로 힌트 — 홈은 ~, 홈 아래는 ~\\…, 밖은 그대로", () => {
  assert.equal(workspaceHintOf("C:\\Users\\u", HOME), "~");
  assert.equal(workspaceHintOf("~", HOME), "~");
  assert.equal(workspaceHintOf("C:\\Users\\u\\proj\\api", HOME), "~\\proj\\api");
  assert.equal(workspaceHintOf("c:\\users\\U\\proj", HOME), "~\\proj"); // 대소문자가 달라도 접두를 알아본다
  assert.equal(workspaceHintOf("D:\\work", HOME), "D:\\work");
  assert.equal(workspaceHintOf("C:\\Users\\u2\\proj", HOME), "C:\\Users\\u2\\proj"); // 접두 글자만 같은 형제 폴더는 홈 아래가 아니다
  assert.equal(workspaceHintOf("C:\\Users\\u\\proj", null), "C:\\Users\\u\\proj");
});

// ── 경로 줄·탭 줄 ────────────────────────────────────────────

test("경로 줄은 탭 목록에 처음 나온 순서, 대소문자만 다른 경로는 하나", () => {
  const tabs = [tab("1", "C:\\b"), tab("2", "C:\\a"), tab("3", "c:\\B\\"), tab("4", "~"), tab("5", "C:\\Users\\u")];
  assert.deepEqual(workspacesOf(tabs, HOME), ["C:\\b", "C:\\a", "~"]);
  assert.deepEqual(workspacesOf([], HOME), []);
});

test("탭 줄은 그 경로의 탭만, 목록 순서대로", () => {
  const tabs = [tab("1", "C:\\a"), tab("2", "C:\\b"), tab("3", "c:/a/")];
  assert.deepEqual(ids(tabsInWorkspace(tabs, "C:\\A", HOME)), ["1", "3"]);
  assert.deepEqual(ids(tabsInWorkspace(tabs, "C:\\none", HOME)), []);
});

test("새 탭 자리 — 같은 경로 탭들의 바로 뒤, 새 경로는 맨 끝", () => {
  const tabs = [tab("1", "C:\\a"), tab("2", "C:\\b"), tab("3", "C:\\a")];
  assert.equal(insertIndexFor(tabs, "c:\\A", HOME), 3); // 마지막 a 탭(자리 2) 뒤
  assert.equal(insertIndexFor(tabs, "C:\\b", HOME), 2);
  assert.equal(insertIndexFor(tabs, "C:\\new", HOME), 3);
  assert.equal(insertIndexFor([], "C:\\x", HOME), 0);
  assert.equal(insertIndexFor(tabs, "~", HOME), 3);
});

test("경로에서 보일 탭 — 마지막에 고른 탭, 닫혔으면 첫 탭, 탭이 없으면 undefined", () => {
  const tabs = [tab("1", "C:\\a"), tab("2", "C:\\a"), tab("3", "C:\\b")];
  const keyA = workspaceKeyOf("C:\\a", HOME);
  assert.equal(shownTabOf(tabs, "C:\\a", { [keyA]: "2" }, HOME), "2");
  assert.equal(shownTabOf(tabs, "C:\\a", { [keyA]: "gone" }, HOME), "1");
  assert.equal(shownTabOf(tabs, "C:\\a", {}, HOME), "1");
  assert.equal(shownTabOf(tabs, "C:\\none", {}, HOME), undefined);
});

// ── 번호·옮기기 ──────────────────────────────────────────────

test("번호 배지 — 수정키를 누른 동안만, 앞 아홉 개만 1..9", () => {
  assert.equal(badgeNumber(false, 0), null);
  assert.deepEqual(Array.from({ length: 12 }, (_, i) => badgeNumber(true, i)), [1, 2, 3, 4, 5, 6, 7, 8, 9, null, null, null]);
});

test("Ctrl+Alt+N — 탭 줄의 N 번째, 탭이 N 보다 적거나 범위 밖이면 아무 일도 없다", () => {
  const list = ["a", "b", "c"];
  assert.equal(nthTab(list, 1), "a");
  assert.equal(nthTab(list, 3), "c");
  assert.equal(nthTab(list, 4), undefined);
  assert.equal(nthTab(list, 9), undefined);
  assert.equal(nthTab(list, 0), undefined);
  assert.equal(nthTab(list, -1), undefined);
  assert.equal(nthTab(list, 1.5), undefined);
  assert.equal(nthTab(list, NaN), undefined);
  assert.equal(nthTab([], 1), undefined);
});

test("Ctrl+Alt+좌우 — 끝에서는 반대쪽 끝으로 돈다", () => {
  assert.equal(wrapStep(3, 0, -1), 2);
  assert.equal(wrapStep(3, 2, 1), 0);
  assert.equal(wrapStep(3, 1, 1), 2);
  assert.equal(wrapStep(3, 1, -1), 0);
  assert.equal(wrapStep(1, 0, 1), 0);
  assert.equal(wrapStep(1, 0, -1), 0);
});

test("옮기기 — 지금 자리를 못 찾으면(-1) 0 에서 센다, 빈 목록은 -1", () => {
  assert.equal(wrapStep(3, -1, 1), 1);
  assert.equal(wrapStep(3, -1, -1), 2);
  assert.equal(wrapStep(0, 0, 1), -1);
  assert.equal(wrapStep(3, 0, -7), 2); // step 이 목록보다 커도 음수 나머지가 새지 않는다
});

// ── 닫은 뒤 고르기 ───────────────────────────────────────────

test("닫은 뒤 — 같은 경로에서 닫힌 자리의 다음 탭, 맨 끝이면 새 마지막", () => {
  const tabs = [tab("a1", "C:\\a"), tab("a2", "C:\\a"), tab("a3", "C:\\a")];
  assert.equal(pickAfterClose(tabs, "a1", {}, HOME), "a2");
  assert.equal(pickAfterClose(tabs, "a2", {}, HOME), "a3");
  assert.equal(pickAfterClose(tabs, "a3", {}, HOME), "a2");
});

test("닫은 뒤 — 자리는 경로 안에서 센다(다른 경로 탭이 사이에 끼어도)", () => {
  const tabs = [tab("a1", "C:\\a"), tab("b1", "C:\\b"), tab("a2", "C:\\a"), tab("b2", "C:\\b"), tab("a3", "C:\\a")];
  assert.equal(pickAfterClose(tabs, "a2", {}, HOME), "a3");
  assert.equal(pickAfterClose(tabs, "a3", {}, HOME), "a2");
  assert.equal(pickAfterClose(tabs, "b1", {}, HOME), "b2");
});

test("닫은 뒤 — 경로의 마지막 탭이면 이웃 경로(뒤 → 없으면 앞)의 마지막에 고른 탭", () => {
  const tabs = [tab("a1", "C:\\a"), tab("b1", "C:\\b"), tab("b2", "C:\\b"), tab("c1", "C:\\c")];
  const lastTab = { [workspaceKeyOf("C:\\b", HOME)]: "b2" };
  assert.equal(pickAfterClose(tabs, "a1", lastTab, HOME), "b2"); // a 가 사라지면 b 경로가 그 자리
  assert.equal(pickAfterClose(tabs, "c1", lastTab, HOME), "b2"); // 맨 끝 경로가 사라지면 앞 경로
  // 마지막에 고른 탭이 없으면 그 경로의 첫 탭
  assert.equal(pickAfterClose(tabs, "a1", {}, HOME), "b1");
});

test("닫은 뒤 — 가운데 경로가 사라지면 그 자리로 올라온 다음 경로", () => {
  const tabs = [tab("a1", "C:\\a"), tab("b1", "C:\\b"), tab("c1", "C:\\c")];
  assert.equal(pickAfterClose(tabs, "b1", {}, HOME), "c1");
});

test("닫은 뒤 — 마지막 탭이면 undefined(빈 상태), 없는 id 도 undefined", () => {
  const tabs = [tab("only", "C:\\a")];
  assert.equal(pickAfterClose(tabs, "only", {}, HOME), undefined);
  assert.equal(pickAfterClose(tabs, "nope", {}, HOME), undefined);
  assert.equal(pickAfterClose([], "x", {}, HOME), undefined);
});

test("닫은 뒤 — 대소문자·슬래시만 다른 같은 경로는 한 경로로 센다", () => {
  const tabs = [tab("1", "C:\\Work\\p"), tab("2", "c:/work/p/"), tab("3", "C:\\other")];
  assert.equal(pickAfterClose(tabs, "1", {}, HOME), "2");
});

test("닫은 뒤 — 홈 탭들(~ 와 전체 경로)은 한 경로", () => {
  const tabs = [tab("h1", "~"), tab("h2", "C:\\Users\\u"), tab("x", "C:\\x")];
  assert.equal(pickAfterClose(tabs, "h1", {}, HOME), "h2");
});

test("닫은 뒤 — 마지막에 고른 탭 기록이 닫는 탭을 가리켜도 쓰지 않는다", () => {
  const tabs = [tab("a1", "C:\\a"), tab("b1", "C:\\b"), tab("b2", "C:\\b")];
  const lastTab = { [workspaceKeyOf("C:\\b", HOME)]: "b2" };
  assert.equal(pickAfterClose(tabs, "b2", lastTab, HOME), "b1");
});

// ── 이름 바꾸기 ──────────────────────────────────────────────

test("이름 바꾸기 — 앞뒤 공백을 떼고 저장", () => {
  assert.equal(renamedTo(null, "proj", "  빌드 서버  "), "빌드 서버");
  assert.equal(renamedTo("old", "proj", "new"), "new");
});

test("이름 바꾸기 — 비우거나 공백뿐이면 사용자 이름을 지운다", () => {
  assert.equal(renamedTo("old", "proj", ""), null);
  assert.equal(renamedTo("old", "proj", "   \t "), null);
  assert.equal(renamedTo(null, "proj", ""), null);
});

test("이름 바꾸기 — 폴더 이름과 같게 저장하면 사용자 이름을 지운다(공백을 떼고 견준다)", () => {
  assert.equal(renamedTo("old", "proj", "proj"), null);
  assert.equal(renamedTo("old", "proj", " proj "), null);
  assert.equal(renamedTo("old", "proj", "Proj"), "Proj"); // 대소문자는 다른 이름
});

test("이름 바꾸기 — 보이던 이름 그대로 저장하면 아무것도 바뀌지 않는다", () => {
  assert.equal(renamedTo("old", "proj", "old"), "old");
  assert.equal(renamedTo("old", "proj", " old "), "old");
  assert.equal(renamedTo(null, "proj", "proj"), null);
});

test("이름 바꾸기 — 한글·이모지·긴 이름은 그대로(길이 제한 없음)", () => {
  assert.equal(renamedTo(null, "proj", "내 프로젝트"), "내 프로젝트");
  assert.equal(renamedTo(null, "proj", "가".repeat(200)), "가".repeat(200));
  assert.equal(renamedTo(null, "proj", "🚀 배포"), "🚀 배포");
});

test("이름 바꾸기 — 홈 탭의 폴더 이름은 ~", () => {
  assert.equal(renamedTo("old", "~", "~"), null);
  assert.equal(renamedTo(null, "~", "홈"), "홈");
});

// ── 닫기 확인·상태 ───────────────────────────────────────────

test("닫기 확인 — 끝나지 않은 세션이 있을 때만 묻고, 셸만 있는 탭·끝난 세션은 바로 닫는다", () => {
  assert.equal(needsCloseConfirm(null), false);
  assert.equal(needsCloseConfirm(undefined), false);
  assert.equal(needsCloseConfirm("ended"), false);
  for (const s of ["working", "waiting", "idle", "paused"]) assert.equal(needsCloseConfirm(s), true, s);
});

test("경로 상태 — 가장 급한 탭 상태, 탭이 없으면 idle, 같은 순위면 앞의 것", () => {
  assert.equal(mostUrgent([]), "idle");
  assert.equal(mostUrgent(["idle", "working", "waiting"]), "waiting");
  assert.equal(mostUrgent(["ended", "idle"]), "idle");
  assert.equal(mostUrgent(["ended"]), "ended");
  assert.equal(mostUrgent(["working", "paused"]), "working");
  assert.equal(mostUrgent(["paused", "idle"]), "paused"); // 순위가 같으면 앞
  assert.equal(mostUrgent(["idle", "paused"]), "idle");
});
