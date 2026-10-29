// 시작 때 한 번 — 백엔드가 옮겨 둔 md-render 폴더를 받아 mdRender.ts 에 쥐여 준다(규칙은 mdRender.ts, 옮기기는 src-tauri/src/mdrender.rs).
import { mdRenderDir } from "$lib/backend";
import { quotePluginDir, setPluginDir } from "./mdRender";

/**
 * 옮겨 둔 폴더를 받아 둔다. 옮기지 못했거나(None) 명령줄에 안전하게 넣을 수 없는 경로면 쥐지 않는다(그러면 `--plugin-dir` 을 붙이지 않는다).
 * 못 쥔 이유는 콘솔에 남긴다 — 세션 띄우기를 막지 않는다
 */
export async function loadPluginDir() {
  try {
    const dir = await mdRenderDir();
    if (dir === null) return;
    quotePluginDir(dir); // 쓸 수 없는 경로면 던진다
    setPluginDir(dir);
  } catch (e) {
    console.error("답변 꾸미기 플러그인 폴더를 쓸 수 없음 — --plugin-dir 을 붙이지 않는다", e);
  }
}
