// 폴더 고르기 대화상자 — Tauri 안에서는 운영체제 대화상자, 브라우저 개발 화면에서는 고정된 가짜 폴더.
// 취소하면 null
import { inTauri } from "$lib/backend";

export async function pickFolder(defaultPath: string): Promise<string | null> {
  let picked: string | string[] | null;
  if (inTauri) {
    const { open } = await import("@tauri-apps/plugin-dialog");
    picked = await open({ directory: true, multiple: false, defaultPath: defaultPath.trim() || undefined });
  } else {
    const { mockPickFolder } = await import("$lib/mock");
    picked = mockPickFolder();
  }
  return typeof picked === "string" ? picked : null;
}
