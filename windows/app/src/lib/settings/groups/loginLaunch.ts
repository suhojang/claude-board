// 설정 > 일반의 "로그인할 때 열기" 스위치 — 바꾼 뒤 실제 값을 다시 읽어 스위치를 맞춘다. 백엔드 호출은 deps 로 받아 node:test 로 시험한다.

export interface LoginLaunchDeps {
  /** 백엔드에 바꾸라고 한다(실패하면 던진다) */
  set(on: boolean): Promise<void>;
  /** 지금 실제로 켜져 있는가 */
  read(): Promise<boolean>;
}

export interface LoginLaunchResult {
  /** 스위치가 보여야 하는 값 */
  value: boolean;
  /** 한 줄 안내 — 바꾸지 못했을 때만 */
  note: string | null;
}

export const LOGIN_LAUNCH_FAILED = "로그인할 때 열기를 바꾸지 못했습니다. 사유는 보드 기록(board.log)에 남습니다";
export const LOGIN_LAUNCH_UNKNOWN = "바뀐 값을 확인하지 못했습니다. 설정을 다시 열어 확인하세요";

/** 스위치를 on 으로 바꾸려 한다. previous 는 누르기 전 값 — 실패하면 그리로 되돌린다 */
export async function changeLoginLaunch(on: boolean, previous: boolean, deps: LoginLaunchDeps): Promise<LoginLaunchResult> {
  try {
    await deps.set(on);
  } catch {
    return { value: previous, note: LOGIN_LAUNCH_FAILED };
  }
  let actual: boolean;
  try {
    actual = await deps.read();
  } catch {
    return { value: previous, note: LOGIN_LAUNCH_UNKNOWN };
  }
  return { value: actual, note: actual === on ? null : LOGIN_LAUNCH_FAILED };
}
