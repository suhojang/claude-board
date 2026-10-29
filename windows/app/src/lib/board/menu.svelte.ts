// 직접 그린 팝업 메뉴 — 계정 메뉴·세션 … 메뉴·경로 카드 오른쪽 클릭 메뉴가 함께 쓴다(macOS NSMenu 자리)
export type MenuItem =
  | { separator: true }
  | {
      label: string;
      action?: () => void;
      enabled?: boolean; // 기본 true
      checked?: boolean;
      header?: boolean; // 누를 수 없는 머리말
      destructive?: boolean;
      title?: string; // 마우스를 올리면 뜨는 설명
      keys?: string; // 오른쪽에 보이는 단축키 글자 — shortcuts/store.svelte.ts 의 keyLabel(id) 로 채워 바꾼 키를 따른다
      submenu?: MenuItem[];
    };

export const menuState = $state({ items: null as MenuItem[] | null, x: 0, y: 0 });

// 마우스 자리(오른쪽 클릭) 또는 단추 아래에 띄운다
export function openMenu(items: MenuItem[], at: MouseEvent | { x: number; y: number }) {
  const p = "clientX" in at ? { x: at.clientX, y: at.clientY } : at;
  menuState.items = items;
  menuState.x = p.x;
  menuState.y = p.y;
}

export function closeMenu() {
  menuState.items = null;
}
