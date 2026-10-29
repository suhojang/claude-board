; Claude Board for Windows — Tauri NSIS 설치 프로그램(setup.exe) hook
; app/src-tauri/tauri.conf.json 의 bundle > windows > nsis > installerHooks 가 이 파일을 가리킨다("../../packaging/nsis-hooks.nsh").
; 설치가 끝나면 claude-board-hook.exe setup 으로 install.ps1 과 같은 사용자 설정(hook 등록·명령·로그인 자동 실행)을 하고,
; 제거할 때는 claude-board-hook.exe uninstall 로 그 설정을 되돌린다(macOS 의 user-setup.sh·Uninstall.command 대응).
; claude-board-hook.exe 는 externalBin(sidecar)으로 묶여 $INSTDIR 에 claude-board-hook.exe 로 설치된다.
; 설치 프로그램은 사용자 단위(installMode currentUser)로 돌아야 한다 — 관리자 권한으로 돌면 setup 이 다른 사용자의 설정에 쓸 수 있다.
; Tauri 가 만드는 NSIS 파일처럼 UTF-8(BOM) 으로 저장한다 — 한글 문구가 깨지지 않게.

!include LogicLib.nsh
!include WinVer.nsh

; ClaudeBoard.exe 는 CreatePseudoConsole(ConPTY)을 정적으로 가져다 쓴다. Windows 10 1809(빌드 17763) 미만에서는
; 실행할 때 진입점 오류만 나므로 설치를 시작하기 전에 막는다. 빌드는 레지스트리 CurrentBuildNumber 를 먼저 본다
; (WinVer 의 GetVersionEx 는 호환 매니페스트가 없으면 거짓 버전을 준다). 못 읽을 때만 WinVer 로 대신한다.
; 조용한 설치(/S)에서도 멈추지 않게 MessageBox 에 /SD IDOK 를 주고 Abort 로 끝낸다.
!macro ClaudeBoardRequireBuild BUILD
  ${If} ${BUILD} < 17763
    MessageBox MB_ICONSTOP|MB_OK "Claude Board 는 Windows 10 1809(빌드 17763) 이상에서만 설치할 수 있습니다.$\r$\n이 PC 의 빌드는 ${BUILD} 입니다. Windows 업데이트 후 다시 실행하세요." /SD IDOK
    Abort
  ${EndIf}
!macroend

!macro ClaudeBoardCheckOsBuild
  Push $0
  ReadRegStr $0 HKLM "SOFTWARE\Microsoft\Windows NT\CurrentVersion" CurrentBuildNumber
  ${If} $0 == ""
    ${WinVerGetBuild} $0
  ${EndIf}
  !insertmacro ClaudeBoardRequireBuild $0
  Pop $0
!macroend

; Tauri 템플릿은 Section WebView2(부트스트래퍼 내려받기) 다음에 Section Install 을 돌리고, NSIS_HOOK_PREINSTALL 은 그 Install 맨 앞에서 불린다.
; 그러면 지원하지 않는 Windows 에서도 WebView2 를 먼저 내려받으므로, 가장 먼저 도는 숨은 Section 에서 검사한다.
Section "-ClaudeBoardOsBuild"
  !insertmacro ClaudeBoardCheckOsBuild
SectionEnd

!macro NSIS_HOOK_POSTINSTALL
  DetailPrint "Claude Board 사용자 설정(hook 등록·명령·자동 실행)"
  Push $0
  nsExec::ExecToLog '"$INSTDIR\claude-board-hook.exe" setup --app "$INSTDIR\ClaudeBoard.exe"'
  Pop $0
  ${If} $0 != 0
    MessageBox MB_ICONEXCLAMATION|MB_OK "Claude Board 사용자 설정(hook 등록)에 실패했습니다($0).$\r$\n$\r$\nPowerShell 에서 다음을 실행해 다시 시도하세요.$\r$\n& $\"$INSTDIR\claude-board-hook.exe$\" setup --app $\"$INSTDIR\ClaudeBoard.exe$\"" /SD IDOK
  ${EndIf}
  Pop $0
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; 새 버전 setup.exe 가 설치 전에 옛 판을 지울 때는 제거 프로그램을 _?=<설치 폴더> 로 제자리에서 돌린다($EXEDIR == $INSTDIR).
  ; 이때 uninstall 을 돌리면 저장한 계정(DPAPI 금고)·hook 까지 지워지므로 건너뛴다 — 곧 이어지는 설치의 setup 이 설정을 다시 쓴다.
  ; 설정 > 앱 에서 제거하거나 uninstall.exe 를 직접 실행하면 NSIS 가 제거 프로그램을 %TEMP% 로 복사해 돌리므로 $EXEDIR 가 다르다.
  ${If} $EXEDIR != $INSTDIR
    DetailPrint "Claude Board 설정 되돌리기(hook·자동 실행·저장한 계정)"
    Push $0
    nsExec::ExecToLog '"$INSTDIR\claude-board-hook.exe" uninstall'
    Pop $0
    Pop $0
  ${EndIf}
!macroend
