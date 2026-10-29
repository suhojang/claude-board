// 화면 나누기 트리의 순수 규칙(mac SplitNode). 노드는 바꾸지 않고 새 트리를 돌려준다. 외부를 import 하지 않는다.
import type { SplitNode } from "../types.ts";

/** 양쪽 화면이 이 길이(px) 아래로 줄지 않게 하는 몫의 하한 — mac setRatio 의 80pt */
export const MIN_PANE_PX = 80;

/** 화면 id 들 — 트리 순서(왼쪽·위가 앞) */
export function leaves(node: SplitNode): string[] {
  return "leaf" in node ? [node.leaf] : [...leaves(node.first), ...leaves(node.second)];
}

/** 그 id 의 화면이 트리에 있는가 */
export function hasLeaf(node: SplitNode, id: string): boolean {
  return "leaf" in node ? node.leaf === id : hasLeaf(node.first, id) || hasLeaf(node.second, id);
}

/** 잎 하나를 다른 노드로 바꾼다(없으면 그대로). 나누기는 잎을 가지로 바꾸는 일이다 */
export function replaceLeaf(node: SplitNode, id: string, by: SplitNode): SplitNode {
  if ("leaf" in node) return node.leaf === id ? by : node;
  return { ...node, first: replaceLeaf(node.first, id, by), second: replaceLeaf(node.second, id, by) };
}

/** 잎을 빼고 부모 자리를 형제가 차지한다. 마지막 화면을 빼면 null */
export function removeLeaf(node: SplitNode, id: string): SplitNode | null {
  if ("leaf" in node) return node.leaf === id ? null : node;
  const first = removeLeaf(node.first, id);
  const second = removeLeaf(node.second, id);
  if (!first) return second;
  if (!second) return first;
  return { ...node, first, second };
}

/** 몫을 고른다 — 양쪽 화면이 MIN_PANE_PX 아래로 줄지 않게(length 는 이 가지의 폭·높이). 길이가 짧으면 0.5 로 모인다 */
export function clampRatio(value: number, length: number, min = MIN_PANE_PX): number {
  const m = Math.min(0.5, min / Math.max(length, 1));
  return Math.min(Math.max(value, m), 1 - m);
}
