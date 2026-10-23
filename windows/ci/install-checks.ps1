# Claude Board for Windows — 실제 Windows 러너에서 설치·hook·재설치·제거를 확인한다 (.github/workflows/windows.yml 의 smoke 작업)
#
#   pwsh windows/ci/install-checks.ps1 -Assets <claude-board-windows 아티팩트 폴더> -Results <smoke-results 폴더>
#
# 러너는 일회용 VM 이라 실제 %USERPROFILE%·HKCU 를 그대로 쓴다 — 개발 기기에서 돌리지 않는다.
# 시나리오(각자 앞 상태가 깨졌으면 스스로 만들고, 끝에 다음 시나리오가 시작할 상태로 정리한다):
#   1) setup.exe /S 무인 설치 — 설치 폴더·hook 11개(exec form)·env·hook 사본·app-path·자동 실행·원래 설정 보존·백업
#   2) 등록된 hook 을 가짜 Claude Code(node.exe 를 claude.exe 로 복사) 아래에서 그대로 실행 — 세션 기록의 state·pid·cwd 등
#   3) 같은 버전 다시 설치·옛 제거 프로그램의 제자리 실행 — 저장한 계정 금고·hook 이 남는지(nsis-hooks.nsh 의 PREUNINSTALL)
#   4) 제거 프로그램 /S — Claude Board 의 것만 지우고 다른 hook·설정·Claude Code 로그인은 남는지
#   5) install.ps1·uninstall.ps1 을 Windows PowerShell 5.1 에서 irm | iex 처럼(웹 호출은 가로챔) - md-render 폴더 교체·빠진 파일 거부·되돌리기 포함
# md-render 플러그인 다섯 파일은 설치 폴더 아래 plugins\md-render\ 에 있어야 하고(setup.exe 와 zip 모두) 제거하면 사라져야 한다
# 확인마다 항목·기대·실제·통과를 표로 남긴다. 실패한 시나리오는 상태를 덤프하고 다음으로 가며, 하나라도 실패하면 끝에 throw 한다.
param(
  [Parameter(Mandatory = $true)][string]$Assets,
  [Parameter(Mandatory = $true)][string]$Results
)

. (Join-Path $PSScriptRoot 'common.ps1')
$CbCurrent.Annotate = 'error'
Start-Transcript -Path (Join-Path $Results 'install-checks.log') -Force | Out-Null
Write-Host "Claude Board for Windows $CbVersion ($CbSemver) 설치 확인 — 자산 $Assets"
Get-ChildItem -LiteralPath $Assets | ForEach-Object { Write-Host ("  {0,-45} {1,12}" -f $_.Name, $_.Length) }
Write-Host "사용자 $env:USERNAME · USERPROFILE $env:USERPROFILE · LOCALAPPDATA $env:LOCALAPPDATA · TEMP $env:TEMP"

try {
  # ------------------------------------------------------------------ 1
  Invoke-Scenario -Name '1) setup.exe /S 무인 설치' -Body {
    Reset-Machine
    $baseline = Get-FileHashText $CbSettings
    $code = Install-WithSetup
    Test-Value 'setup.exe /S 종료 코드' 0 $code
    if ($code -ne 0) { throw 'setup.exe /S 가 실패해 이 시나리오의 나머지 확인을 건너뜁니다.' }

    $inst = Get-InstallLocation
    Test-Value '설치 폴더(Uninstall 키의 InstallLocation) = %LOCALAPPDATA%\Claude Board (installMode currentUser)' $CbSetupDir $inst
    foreach ($f in @('ClaudeBoard.exe', 'claude-board-hook.exe', 'uninstall.exe')) {
      Test-Value "설치 폴더에 $f" $true (Test-Path -LiteralPath (Join-Path $CbSetupDir $f) -PathType Leaf)
    }
    Test-PluginFiles $CbSetupDir '설치 폴더'
    Test-Value '설치 폴더에 THIRD_PARTY_NOTICES.txt' $true (Test-Path -LiteralPath (Join-Path $CbSetupDir 'THIRD_PARTY_NOTICES.txt') -PathType Leaf) -Info
    Test-Value '설정 > 앱 의 표시 버전(DisplayVersion)' $CbSemver (Get-RegValue $CbUninstKey 'DisplayVersion')

    $ver = Invoke-Native -File (Join-Path $CbSetupDir 'claude-board-hook.exe') -Arguments @('--version') -TimeoutSec 30
    Test-Value 'claude-board-hook.exe --version' "claude-board-hook $CbVersion" ("$($ver.Out)".Trim())

    Test-HookRegistration
    Test-PreservedSettings
    $backups = Get-SettingsBackups
    Test-Value 'settings.json 백업(settings.json.bak-claude-board-*)이 생김' $true ($backups.Count -ge 1)
    if ($backups.Count -ge 1) {
      Test-Value '백업 내용 = 설치 전 settings.json' $baseline (Get-FileHashText $backups[0].FullName)
    }

    Test-Value 'hook 사본 %USERPROFILE%\.claude\notifier\claude-board-hook.exe = 설치 폴더의 원본' (Get-FileHashText (Join-Path $CbSetupDir 'claude-board-hook.exe')) (Get-FileHashText $CbHookCopy)
    $appPath = if (Test-Path -LiteralPath $CbAppPath) { (Read-TextShared $CbAppPath).Trim() } else { $null }
    Test-Value 'notifier\app-path' (Join-Path $CbSetupDir 'ClaudeBoard.exe') $appPath
    Test-Value "로그인 자동 실행 HKCU\...\Run 의 $CbRunValue" ('"{0}" --background' -f (Join-Path $CbSetupDir 'ClaudeBoard.exe')) (Get-RunValue)
    foreach ($n in $CbCmdFiles) {
      Test-Value "명령 %USERPROFILE%\.local\bin\$n" $true (Test-Path -LiteralPath (Join-Path $CbLocalBin $n) -PathType Leaf)
    }
    Test-Value 'Claude Code 로그인(.credentials.json)은 그대로' $CbBaselineCredentials (Read-TextShared $CbCredentials)
    Test-Value '설치 뒤 떠 있는 보드(/S 는 띄우지 않음)' 0 (@(Get-BoardProcesses).Count) -Info
  } -Cleanup {
    Stop-ClaudeBoardProcesses
  }

  # ------------------------------------------------------------------ 2
  Invoke-Scenario -Name '2) 등록된 hook 을 가짜 Claude Code 아래에서 실행' -Body {
    Initialize-SetupInstall
    $proj = New-Project 'demo-proj'
    $fake = Start-FakeClaude -WorkDir $proj
    $CbState.S2Fake = $fake
    Test-Value '가짜 claude.exe 가 스스로 알린 pid = 띄운 프로세스 pid' $fake.Pid $fake.Ready.pid
    $sid = 'ci-s2-' + [guid]::NewGuid().ToString('N').Substring(0, 8)
    $boardsBefore = @(Get-BoardProcesses | ForEach-Object { $_.Pid })

    # SessionStart — 처음 보는 세션: Claude 프로세스(조상의 claude.exe)를 찾아 pid·생성 시각·headless 를 남긴다
    $r = Send-HookEvent $fake 'SessionStart' $sid $proj @{ source = 'startup'; model = 'claude-opus-5-5' }
    Test-HookResults $r
    $rec = Read-Session $sid
    Test-Value 'SessionStart 뒤 세션 파일이 생김' $true ($null -ne $rec)
    if ($null -eq $rec) { throw "세션 파일 $CbSessions\$sid.json 이 없습니다." }
    Test-Value 'state (SessionStart)' 'idle' $rec.state
    Test-Value 'pid = 가짜 claude.exe 의 pid' $fake.Pid $rec.pid
    $startUnix = [DateTimeOffset]::new($fake.Process.StartTime).ToUnixTimeMilliseconds() / 1000.0
    $ctime = if ($null -ne $rec.pid_ctime) { [double]$rec.pid_ctime } else { 0 }
    $diff = [Math]::Abs($ctime - $startUnix)
    Add-Check -Item 'pid_ctime = 가짜 claude.exe 의 생성 시각(±2초, pid 재사용 구분)' -Expected ('{0:F3}' -f $startUnix) -Actual ('{0} (차이 {1:F3}초)' -f (Format-Cell $rec.pid_ctime), $diff) -Pass ($diff -lt 2)
    Test-Value 'session_id' $sid $rec.session_id
    Test-Value 'cwd' $proj $rec.cwd
    Test-Value 'project' 'demo-proj' $rec.project
    Test-Value 'tty (Windows 는 늘 null)' $null $rec.tty
    Test-Value 'model (SessionStart 입력)' 'claude-opus-5-5' $rec.model
    Test-Value 'transcript_path' (New-Transcript -Cwd $proj -SessionId $sid) $rec.transcript_path
    Test-Value 'headless (새 콘솔 창에서 띄운 세션 — SPEC 3.4 추정 규칙, Windows 미확인)' $false $rec.headless -Info
    Test-Value 'args (carry_args — 가짜 claude 인자에는 넘길 것이 없음)' '[]' (ConvertTo-Json -InputObject @($rec.args) -Compress) -Info

    # UserPromptSubmit — 작업 중 + topic. 이때 콘솔 제목도 바뀐다(Windows 미확인 — 정보로만)
    $prompt = 'CI  확인용 요청 — 설치된 hook 이   세션 기록을 남기는지 봅니다'
    $r = Send-HookEvent $fake 'UserPromptSubmit' $sid $proj @{ prompt = $prompt; permission_mode = 'default' } -ReadTitle
    Test-HookResults $r
    $rec = Read-Session $sid
    Test-Value 'state (UserPromptSubmit)' 'working' $rec.state
    $collapsed = Format-CollapsedWhitespace $prompt
    $topic = $collapsed.Substring(0, [Math]::Min(40, $collapsed.Length))
    Test-Value 'topic (공백 정리 후 앞 40자)' $topic $rec.topic
    $tail = if ($topic.Length -gt 20) { ' · ' + $topic.Substring(0, 20) + '…' } else { ' · ' + $topic }
    $wantTitle = [char]::ConvertFromUtf32(0x1F535) + ' demo-proj' + $tail
    Add-Check -Item '콘솔 제목(hook 이 AttachConsole 로 바꿈 — headless 면 안 바꿈, Windows 미확인)' -Expected $wantTitle -Actual ("{0} (headless {1})" -f (Format-Cell $r.title), (Format-Cell $rec.headless)) -Pass ("$($r.title)" -ceq $wantTitle) -Info

    # PreToolUse — 계속 작업 중
    $r = Send-HookEvent $fake 'PreToolUse' $sid $proj @{ tool_name = 'Bash'; tool_input = @{ command = 'dir' }; tool_use_id = 'toolu_ci_1' }
    Test-HookResults $r
    Test-Value 'state (PreToolUse)' 'working' (Read-Session $sid).state

    # Notification(permission_prompt) — notify(패널을 분리 프로세스로 띄우고 기다리지 않음) + 승인 대기
    $message = 'Claude needs your permission to use Bash'
    $r = Send-HookEvent $fake 'Notification' $sid $proj @{ message = $message; notification_type = 'permission_prompt' }
    Test-HookResults $r
    $rec = Read-Session $sid
    Test-Value 'state (Notification)' 'waiting' $rec.state
    Test-Value 'last_message (승인 대기 메시지)' $message $rec.last_message

    # Stop — notify(작업 완료) + 대기, last_message 는 공백 정리
    $last = "CI 작업을   마쳤습니다.`n다음 단계는 없습니다."
    $r = Send-HookEvent $fake 'Stop' $sid $proj @{ stop_hook_active = $false; last_assistant_message = $last }
    Test-HookResults $r
    $rec = Read-Session $sid
    Test-Value 'state (Stop)' 'idle' $rec.state
    Test-Value 'last_message (공백 정리)' (Format-CollapsedWhitespace $last) $rec.last_message
    $ra = @($rec.resume_args)
    $raTail = if ($ra.Count -ge 2) { "$($ra[$ra.Count - 2]) $($ra[$ra.Count - 1])" } else { "$ra" }
    Test-Value 'resume_args 끝 = --resume <세션 id>' "--resume $sid" $raTail
    Test-Value 'resume_cmd(표시용)' "claude ... --resume $sid" $rec.resume_cmd -Info
    $panels = @(Get-BoardProcesses | Where-Object { $boardsBefore -notcontains $_.Pid -and "$($_.CommandLine)" -match '--notify' })
    Test-Value 'notify hook 이 띄운 알림 패널 프로세스 수(Notification·Stop)' 2 $panels.Count -Info

    # SessionEnd — 종료
    $r = Send-HookEvent $fake 'SessionEnd' $sid $proj @{ reason = 'other' }
    Test-HookResults $r
    Test-Value 'state (SessionEnd)' 'ended' (Read-Session $sid).state
    Show-File (Join-Path $fake.Dir 'fake-claude.log')
  } -Cleanup {
    Stop-FakeClaude $CbState.S2Fake
    Stop-ClaudeBoardProcesses
    Remove-PathRetry $CbSessions
    New-Item -ItemType Directory -Force -Path $CbSessions | Out-Null
  }

  # ------------------------------------------------------------------ 3
  Invoke-Scenario -Name '3) 같은 버전 다시 설치(업데이트 흉내)·옛 제거 프로그램의 제자리 실행' -Body {
    Initialize-SetupInstall
    $vault = Join-Path $CbNotifier 'accounts\vault'
    $marker = Join-Path $vault 'ci-marker.bin'
    Write-Utf8 $marker 'CI 표지 — 저장한 계정 금고 흉내'
    Write-Utf8 (Join-Path $CbNotifier 'accounts.json') '{"accounts":[]}'
    Write-Utf8 (Join-Path $CbNotifier 'notify-position') 'top-right'
    # Uninstall 키는 Tauri 제거 프로그램이 통째로 지운다 — 이 표지가 사라지면 옛 제거 프로그램이 돈 것이다
    Set-ItemProperty -LiteralPath $CbUninstKey -Name 'CiMarker' -Value 'smoke'

    # (가) setup.exe /S 를 다시 — 도는 동안 제거 프로그램 프로세스가 보이는지 살핀다
    Write-Host "  실행: `"$CbSetupExe`" /S (제거 프로그램 프로세스를 살피며)"
    $p = Start-Process -FilePath $CbSetupExe -ArgumentList '/S' -PassThru
    $null = $p.Handle
    $seen = [System.Collections.Generic.HashSet[string]]::new()
    $deadline = [DateTime]::UtcNow.AddSeconds(300)
    while (-not $p.HasExited) {
      foreach ($q in @(Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.ProcessName -match '^(uninstall|Un_[A-Z]|Au_)$' })) {
        [void]$seen.Add("$($q.ProcessName)($($q.Id))")
      }
      if ([DateTime]::UtcNow -gt $deadline) { Stop-Process -Id $p.Id -Force; throw 'setup.exe /S(다시 설치)가 300초 안에 끝나지 않았습니다.' }
      Start-Sleep -Milliseconds 50
    }
    $p.WaitForExit()
    Test-Value 'setup.exe /S(다시 설치) 종료 코드' 0 $p.ExitCode
    $called = $null -eq (Get-RegValue $CbUninstKey 'CiMarker')
    Test-Value '다시 설치가 옛 제거 프로그램을 불렀는가(Uninstall 키 표지가 사라짐)' $false $called -Info
    Test-Value '다시 설치 중 본 제거 프로그램 프로세스' '' (@($seen) -join ', ') -Info
    if (-not $called) {
      Write-Host '  ※ Tauri NSIS 는 /S(무인)에서 "다시 설치" 쪽지(PageReinstall)를 건너뛰어 옛 제거 프로그램을 부르지 않는다.'
      Write-Host '    그래서 (가)의 "표지·hook 이 남는다" 는 확인은 의미가 약하다 — (나)에서 GUI 업그레이드가 쓰는 제자리 제거(_?=)를 직접 돌린다.'
    }
    Test-Value '(가) 저장한 계정 금고 표지가 남음' $true (Test-Path -LiteralPath $marker)
    Test-Value '(가) accounts.json 이 남음' $true (Test-Path -LiteralPath (Join-Path $CbNotifier 'accounts.json'))
    Test-Value '(가) notify-position 이 남음' $true (Test-Path -LiteralPath (Join-Path $CbNotifier 'notify-position'))
    Test-Value '(가) Claude Board hook 명령 수' $CbHookEntryCount (Get-OurHookEntryCount)
    Test-Value '(가) 자동 실행 값' ('"{0}" --background' -f (Join-Path $CbSetupDir 'ClaudeBoard.exe')) (Get-RunValue)
    Test-Value '(가) 설치 폴더에 ClaudeBoard.exe' $true (Test-Path -LiteralPath (Join-Path $CbSetupDir 'ClaudeBoard.exe'))
    Test-PluginFiles $CbSetupDir '(가) 설치 폴더'

    # (나) 새 버전 setup.exe 를 GUI 로 돌리면 PageLeaveReinstall 이 옛 제거 프로그램을 `"<설치 폴더>\uninstall.exe" _?=<설치 폴더>` 로 제자리에서 부른다.
    # 그때 $EXEDIR == $INSTDIR 이라 PREUNINSTALL 이 claude-board-hook uninstall 을 건너뛰어야 한다 — 같은 명령줄을 직접 돌린다
    $inst = Get-InstallLocation
    $uninst = Join-Path $inst 'uninstall.exe'
    $code = Start-AndWait -File $uninst -ArgumentString "/S _?=$inst" -TimeoutSec 120
    Test-Value '(나) 제자리 제거 종료 코드' 0 $code
    Test-Value '(나) 제거 프로그램이 돌았음 — 설치 폴더의 ClaudeBoard.exe 가 지워짐' $false (Test-Path -LiteralPath (Join-Path $inst 'ClaudeBoard.exe'))
    Test-Value '(나) 설치 폴더의 claude-board-hook.exe 가 지워짐' $false (Test-Path -LiteralPath (Join-Path $inst 'claude-board-hook.exe'))
    Test-PluginFiles $inst '(나) 지워진 설치 폴더' -Expect $false
    Test-Value '(나) uninstall.exe 는 제자리라 남음' $true (Test-Path -LiteralPath $uninst) -Info
    Test-Value '(나) Uninstall 키가 지워짐' $false (Test-Path -LiteralPath $CbUninstKey) -Info
    Test-Value '(나) PREUNINSTALL 이 건너뜀 — 저장한 계정 금고 표지가 남음' $true (Test-Path -LiteralPath $marker)
    Test-Value '(나) accounts.json 이 남음' $true (Test-Path -LiteralPath (Join-Path $CbNotifier 'accounts.json'))
    Test-Value '(나) notify-position 이 남음' $true (Test-Path -LiteralPath (Join-Path $CbNotifier 'notify-position'))
    Test-Value '(나) Claude Board hook 명령 수' $CbHookEntryCount (Get-OurHookEntryCount)
    Test-Value '(나) 자동 실행 값이 남음' ('"{0}" --background' -f (Join-Path $CbSetupDir 'ClaudeBoard.exe')) (Get-RunValue)
    Test-Value '(나) hook 사본이 남음' $true (Test-Path -LiteralPath $CbHookCopy)
    Test-Value '(나) app-path 가 남음' $true (Test-Path -LiteralPath $CbAppPath)

    # (다) 이어지는 설치 — 같은 자리(HKCU\Software\claude\Claude Board 의 기본값)에 다시 깔린다
    $code = Install-WithSetup
    Test-Value '(다) 이어지는 setup.exe /S 종료 코드' 0 $code
    Test-Value '(다) 같은 설치 폴더' $CbSetupDir (Get-InstallLocation)
    Test-Value '(다) 설치 폴더에 ClaudeBoard.exe' $true (Test-Path -LiteralPath (Join-Path $CbSetupDir 'ClaudeBoard.exe'))
    Test-PluginFiles $CbSetupDir '(다) 설치 폴더'
    Test-Value '(다) Claude Board hook 명령 수' $CbHookEntryCount (Get-OurHookEntryCount)
    Test-Value '(다) 저장한 계정 금고 표지가 남음' $true (Test-Path -LiteralPath $marker)
  } -Cleanup {
    Stop-ClaudeBoardProcesses
    Remove-PathRetry (Join-Path $CbNotifier 'accounts')
    Remove-PathRetry (Join-Path $CbNotifier 'accounts.json')
    Remove-PathRetry (Join-Path $CbNotifier 'notify-position')
    Remove-ItemProperty -LiteralPath $CbUninstKey -Name 'CiMarker' -ErrorAction SilentlyContinue
  }

  # ------------------------------------------------------------------ 4
  Invoke-Scenario -Name '4) 제거 프로그램 /S' -Body {
    Initialize-SetupInstall
    # 제거가 지워야 하는 것(SPEC 3.7) — 저장한 계정·알림 위치·세션 기록
    Write-Utf8 (Join-Path $CbNotifier 'accounts\vault\ci-marker.bin') 'CI 표지'
    Write-Utf8 (Join-Path $CbNotifier 'accounts.json') '{"accounts":[]}'
    Write-Utf8 (Join-Path $CbNotifier 'notify-position') 'top-right'
    Write-Utf8 (Join-Path $CbSessions 'ci-old.json') '{"session_id":"ci-old","state":"ended"}'
    # 보드가 옮겨 쓴 md-render 사본 - 제거가 지워야 한다(claude-board-hook uninstall 이 notifier\plugins 를 지움)
    New-PluginCopy
    $backupsBefore = (Get-SettingsBackups).Count
    $inst = Get-InstallLocation

    # _?= 없이 /S — NSIS 제거 프로그램은 자신을 %TEMP% 로 복사해 돌리고 바로 돌아온다. 끝날 때까지 살핀다
    $code = Start-AndWait -File (Join-Path $inst 'uninstall.exe') -ArgumentString '/S' -TimeoutSec 60
    Test-Value 'uninstall.exe /S 종료 코드(복사본을 띄우고 바로 돌아옴)' 0 $code
    $seen = [System.Collections.Generic.HashSet[string]]::new()
    $done = Wait-Until -TimeoutSec 90 -IntervalMs 250 -Condition {
      foreach ($q in @(Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.ProcessName -match '^(Un_[A-Z]|Au_)$' })) { [void]$seen.Add("$($q.ProcessName)($($q.Id))") }
      $running = @(Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.ProcessName -match '^(Un_[A-Z]|Au_)$' }).Count
      ($running -eq 0) -and -not (Test-Path -LiteralPath $inst) -and -not (Test-Path -LiteralPath $CbUninstKey)
    }
    Test-Value '제거가 90초 안에 끝남(제거 프로그램 사본 종료·설치 폴더·Uninstall 키 사라짐)' $true ([bool]$done)
    Test-Value '본 제거 프로그램 사본 프로세스' '' (@($seen) -join ', ') -Info
    Test-Value '설치 폴더가 지워짐' $false (Test-Path -LiteralPath $inst)
    Test-PluginFiles $inst '제거 뒤 설치 폴더' -Expect $false
    Test-Value 'notifier\plugins\md-render 사본이 지워짐' $false (Test-Path -LiteralPath $CbPluginCopy)
    Test-Value 'Uninstall 키가 지워짐' $false (Test-Path -LiteralPath $CbUninstKey)
    # NSIS 는 Run 의 "Claude Board" 만 지운다 — "ClaudeBoard" 가 사라졌다면 PREUNINSTALL 의 claude-board-hook uninstall 이 돈 것이다
    Test-Value "자동 실행 값 $CbRunValue 가 지워짐(= hook uninstall 이 돌았음)" $null (Get-RunValue)
    Test-HooksRemoved
    Test-PreservedSettings
    Test-Value 'hook 을 빼기 전 settings.json 백업이 새로 생김' $true ((Get-SettingsBackups).Count -gt $backupsBefore) -Info
    Test-Value 'notifier 에서 지워야 할 것 중 남은 것(hook 사본·app-path·notify-position·sessions·accounts.json·accounts·plugins)' '' (@(Get-NotifierLeftovers) -join ', ')
    $rest = if (Test-Path -LiteralPath $CbNotifier) { (@(Get-ChildItem -LiteralPath $CbNotifier -Force | ForEach-Object { $_.Name }) -join ', ') } else { '(폴더 없음)' }
    Test-Value 'notifier 폴더에 남은 것(목록에 없는 board.log 등은 남을 수 있다)' '(폴더 없음)' $rest -Info
    foreach ($n in $CbCmdFiles) {
      Test-Value "명령 $n 이 지워짐" $false (Test-Path -LiteralPath (Join-Path $CbLocalBin $n))
    }
    Test-Value 'Claude Code 로그인(.credentials.json)은 그대로' $CbBaselineCredentials (Read-TextShared $CbCredentials)
    Test-Value '떠 있는 ClaudeBoard.exe' 0 (@(Get-BoardProcesses).Count)
    $canonical = { param($t) ConvertTo-Json -InputObject ($t | ConvertFrom-Json -Depth 64) -Depth 64 -Compress }
    Test-Value 'settings.json 이 설치 전과 같음(순서 포함)' (& $canonical $CbBaselineSettings) (& $canonical (Read-TextShared $CbSettings)) -Info
  } -Cleanup {
    Reset-Machine
  }

  # ------------------------------------------------------------------ 5
  Invoke-Scenario -Name '5) install.ps1·uninstall.ps1 (Windows PowerShell 5.1, irm | iex)' -Body {
    Reset-Machine
    $installPs1 = Join-Path $Assets 'install.ps1'
    $uninstallPs1 = Join-Path $Assets 'uninstall.ps1'
    $runPs51 = {
      param([string]$Mode, [string]$Script, [string]$AssetsDir = '')
      $assetsArg = if ($AssetsDir) { $AssetsDir } else { $Assets }
      $resultFile = Join-Path $CbTemp "ps51-$Mode.json"
      Remove-Item -LiteralPath $resultFile -ErrorAction SilentlyContinue
      $ps = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
      $driver = Join-Path $PSScriptRoot 'ps51-install.ps1'
      # 보드의 업데이트 단추처럼 PSModulePath 를 지우고 띄운다(updater.rs) — 이 pwsh 의 PowerShell 7 모듈 경로를 물려주지 않는다
      $r = Invoke-Native -File $ps -Arguments @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $driver, '-Mode', $Mode, '-Script', $Script, '-Assets', $assetsArg, '-CiVersion', $CbVersion, '-Result', $resultFile) -TimeoutSec 300 -UnsetEnv 'PSModulePath'
      Write-Host "  --- powershell.exe ($Mode) 종료 코드 $(Format-Cell $r.ExitCode), 시간 초과 $($r.TimedOut) ---"
      foreach ($l in ("$($r.Out)" -split "`r?`n")) { Write-Host "    > $l" }
      if ($r.Err) { foreach ($l in ("$($r.Err)" -split "`r?`n")) { Write-Host "    ! $l" } }
      if (-not (Test-Path -LiteralPath $resultFile)) {
        Add-Check -Item "($Mode) 실행기가 결과를 남김(스크립트가 exit 로 셸을 끝내지 않음)" -Expected '결과 파일 있음' -Actual '없음' -Pass $false
        return $null
      }
      return ((Read-TextShared $resultFile) | ConvertFrom-Json)
    }

    # PowerShell 7 의 PSModulePath 를 물려받은 5.1 은 PowerShell 7 모듈을 읽으려다 Get-FileHash(스크립트 모듈 명령)를 찾지 못한다.
    # 보드가 PowerShell 7 에서 띄워졌을 때의 업데이트 단추가 이 경우였다 — 그래서 updater.rs 가 PSModulePath 를 지우고 띄운다
    $ps = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
    $probe = @('-NoProfile', '-Command', "if (Get-Command Get-FileHash -ErrorAction SilentlyContinue) { 'found' } else { 'missing' }")
    $inherited = Invoke-Native -File $ps -Arguments $probe -TimeoutSec 60
    Test-Value 'PowerShell 7 의 PSModulePath 를 물려받은 5.1 의 Get-FileHash(원인 기록)' 'missing' "$($inherited.Out)".Trim() -Info
    $cleaned = Invoke-Native -File $ps -Arguments $probe -TimeoutSec 60 -UnsetEnv 'PSModulePath'
    Test-Value 'PSModulePath 를 지우고 띄운 5.1 의 Get-FileHash' 'found' "$($cleaned.Out)".Trim()

    # (a) 처음 설치 + 표준 입력 리디렉션 — hook 등록을 물을 수 없으므로 안내와 함께 throw, 아무것도 설치하지 않는다
    $settingsBefore = Get-FileHashText $CbSettings
    $o = & $runPs51 'first' $installPs1
    if ($o) {
      Test-Value '(a) Windows PowerShell 버전' '5.1' ($o.ps -replace '^(\d+\.\d+).*$', '$1')
      Test-Value '(a) 표준 입력이 리디렉션됨(전제)' $true $o.inputRedirected
      Test-Value '(a) throw 함' $true $o.threw
      Add-Check -Item '(a) 안내: 터미널에서 실행해야 한다' -Expected '…터미널(PowerShell 창)에서 설치 명령을 실행해야…' -Actual $o.error -Pass ("$($o.error)" -like '*터미널*')
      Test-Value '(a) 웹 호출 없음' '' (@($o.calls) -join ' / ')
    }
    Test-Value '(a) 앱 폴더를 만들지 않음' $false (Test-Path -LiteralPath $CbScriptDir)
    Test-Value '(a) settings.json 이 그대로' $settingsBefore (Get-FileHashText $CbSettings)
    Test-Value '(a) notifier 폴더를 만들지 않음' $false (Test-Path -LiteralPath $CbNotifier)

    # (b) -Update(묻지 않음), 버전은 릴리스 목록에서 — macOS 판 Latest(v 태그)·draft·prerelease 를 건너뛰고 windows-v<버전> 을 고른다
    $o = & $runPs51 'update' $installPs1
    if ($o) {
      Test-Value '(b) throw 하지 않음' $null $o.error
      $calls = @($o.calls)
      Test-Value '(b) 릴리스 목록을 한 번 읽음' 1 @($calls | Where-Object { $_ -like 'Invoke-RestMethod *' }).Count
      $dl = @($calls | Where-Object { $_ -like 'Invoke-WebRequest *' })
      Test-Value '(b) 받은 것: windows-v<버전> 의 zip·sha256' ("windows-v$CbVersion/ClaudeBoard-windows-x64.zip, windows-v$CbVersion/ClaudeBoard-windows-x64.zip.sha256") (($dl | ForEach-Object { ($_ -split '/download/', 2)[1] }) -join ', ')
    }
    # 폴더 안 파일까지 상대 경로·해시(·수정 시각)를 한 줄로 - zip 을 푼 폴더와 앱 폴더를 비교한다
    $tree = {
      param([string]$Root, [bool]$Times = $false)
      if (-not (Test-Path -LiteralPath $Root)) { return '(폴더 없음)' }
      $r = $Root.TrimEnd('\')
      return (@(Get-ChildItem -LiteralPath $Root -Recurse -File -Force | Sort-Object FullName | ForEach-Object {
        $t = if ($Times) { "@$($_.LastWriteTimeUtc.Ticks)" } else { '' }
        "$($_.FullName.Substring($r.Length + 1))=$((Get-FileHash -LiteralPath $_.FullName).Hash)$t"
      }) -join ', ')
    }
    $expected = Join-Path $CbTemp 'zip-expected'
    Remove-PathRetry $expected
    Expand-Archive -LiteralPath (Join-Path $Assets 'ClaudeBoard-windows-x64.zip') -DestinationPath $expected -Force
    $want = & $tree $expected
    $have = & $tree $CbScriptDir
    Test-Value '(b) %LOCALAPPDATA%\Programs\Claude Board 의 파일 = zip 내용(plugins\md-render 포함, .old-* 없음)' $want $have
    Test-PluginFiles $CbScriptDir '(b) 앱 폴더'
    Test-HookRegistration
    Test-PreservedSettings
    $appPath = if (Test-Path -LiteralPath $CbAppPath) { (Read-TextShared $CbAppPath).Trim() } else { $null }
    Test-Value '(b) app-path' (Join-Path $CbScriptDir 'ClaudeBoard.exe') $appPath
    Test-Value '(b) hook 사본 = 앱 폴더의 원본' (Get-FileHashText (Join-Path $CbScriptDir 'claude-board-hook.exe')) (Get-FileHashText $CbHookCopy)
    Test-Value '(b) 자동 실행 값' ('"{0}" --background' -f (Join-Path $CbScriptDir 'ClaudeBoard.exe')) (Get-RunValue)
    foreach ($n in $CbCmdFiles) {
      Test-Value "(b) 명령 $n" $true (Test-Path -LiteralPath (Join-Path $CbLocalBin $n) -PathType Leaf)
    }
    $boardExe = Join-Path $CbScriptDir 'ClaudeBoard.exe'
    $board = Wait-Until -TimeoutSec 20 -Condition { @(Get-BoardProcesses | Where-Object { $_.Path -eq $boardExe -and "$($_.CommandLine)" -notmatch '--notify' }) | Select-Object -First 1 }
    Test-Value '(b) 보드가 떠 있음' $boardExe $(if ($board) { $board.Path } else { $null })

    # (c) .sha256 이 틀리면 throw 하고 이미 깔린 파일·떠 있는 보드는 그대로(해시 확인이 보드를 끄기 전에 있다)
    $snap = { & $tree $CbScriptDir $true }
    $before = & $snap
    $o = & $runPs51 'badsha' $installPs1
    if ($o) {
      Test-Value '(c) throw 함' $true $o.threw
      Add-Check -Item '(c) 안내: SHA256 이 다르다' -Expected '…SHA256 이 릴리스에 적힌 값과 달라…' -Actual $o.error -Pass ("$($o.error)" -like '*SHA256*')
    }
    Test-Value '(c) 앱 폴더 파일이 그대로(내용·수정 시각)' $before (& $snap)
    if ($board) { Test-Value '(c) 보드가 그대로 떠 있음' $true (Test-ProcessAlive $board.Pid) }

    # 변형한 zip 으로 묶은 자산 폴더 - $Assets 를 복사하고 zip 만 $Mutate 로 고쳐 다시 묶는다(.sha256 도 새로 계산)
    $makeAssets = {
      param([string]$Name, [scriptblock]$Mutate)
      $a = Join-Path $CbTemp $Name
      $x = Join-Path $CbTemp "$Name-x"
      Remove-PathRetry $a
      Remove-PathRetry $x
      Copy-Item -LiteralPath $Assets -Destination $a -Recurse
      Expand-Archive -LiteralPath (Join-Path $Assets 'ClaudeBoard-windows-x64.zip') -DestinationPath $x -Force
      & $Mutate $x
      $zip = Join-Path $a 'ClaudeBoard-windows-x64.zip'
      Remove-Item -LiteralPath $zip -Force
      Compress-Archive -Path (Join-Path $x '*') -DestinationPath $zip
      $h = (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash.ToLowerInvariant()
      [IO.File]::WriteAllText("$zip.sha256", "$h  ClaudeBoard-windows-x64.zip`n", [Text.UTF8Encoding]::new($false))
      return $a
    }

    # (d) zip 에 플러그인 파일이 빠져 있으면 아무것도 건드리기 전에 throw - 깔린 파일·떠 있는 보드는 그대로
    $noFile = & $makeAssets 'assets-nofile' { param($x) Remove-Item -LiteralPath (Join-Path $x 'plugins\md-render\hooks\register.tsx') -Force }
    $before = & $snap
    $o = & $runPs51 'update' $installPs1 $noFile
    if ($o) {
      Test-Value '(d) throw 함' $true $o.threw
      Add-Check -Item '(d) 안내: zip 에 hooks\register.tsx 가 없다' -Expected '…받은 zip 에 plugins\md-render\hooks\register.tsx 가 없습니다.' -Actual $o.error -Pass ("$($o.error)" -like '*plugins\md-render\hooks\register.tsx*')
    }
    Test-Value '(d) 앱 폴더 파일이 그대로(내용·수정 시각)' $before (& $snap)
    if ($board) { Test-Value '(d) 보드가 그대로 떠 있음' $true (Test-ProcessAlive $board.Pid) }

    # (e) 바꿔 끼우는 도중 플러그인 폴더가 막혀 실패하면 이미 바꾼 최상위 파일까지 모두 되돌린다 - 열어 둔 파일 하나가 폴더 이름 바꾸기를 막는다.
    # zip 의 README.md(최상위, 먼저 바뀜)와 플러그인 README.md 를 다르게 만들어 두어 되돌림이 없으면 눈에 띄게 한다
    $changed = & $makeAssets 'assets-changed' {
      param($x)
      Add-Content -LiteralPath (Join-Path $x 'README.md') -Value 'CI 변경'
      Add-Content -LiteralPath (Join-Path $x 'plugins\md-render\README.md') -Value 'CI 변경'
    }
    $before = & $snap
    $lockFile = Join-Path $CbScriptDir 'plugins\md-render\hooks\register.tsx'
    $held = [IO.File]::Open($lockFile, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::None)
    try {
      $o = & $runPs51 'update' $installPs1 $changed
    } finally {
      $held.Dispose()
    }
    if ($o) {
      Test-Value '(e) throw 함' $true $o.threw
      Add-Check -Item '(e) 안내: 원래대로 되돌렸다' -Expected '…앱 파일을 바꿔 넣지 못해 원래대로 되돌렸습니다…' -Actual $o.error -Pass ("$($o.error)" -like '*원래대로 되돌렸습니다*')
    }
    Test-Value '(e) 앱 폴더 파일이 처음과 같음(내용·수정 시각, .old-* 없음)' $before (& $snap)

    # uninstall.ps1 — irm | iex. 보드가 옮겨 쓴 md-render 사본(notifier\plugins\md-render)도 지워야 한다
    New-PluginCopy
    $o = & $runPs51 'uninstall' $uninstallPs1
    if ($o) { Test-Value 'uninstall.ps1 throw 하지 않음' $null $o.error }
    Test-Value 'uninstall.ps1: 앱 폴더가 지워짐' $false (Test-Path -LiteralPath $CbScriptDir)
    Test-PluginFiles $CbScriptDir 'uninstall.ps1: 지워진 앱 폴더' -Expect $false
    Test-Value 'uninstall.ps1: notifier\plugins\md-render 사본이 지워짐' $false (Test-Path -LiteralPath $CbPluginCopy)
    Test-HooksRemoved
    Test-PreservedSettings
    Test-Value 'uninstall.ps1: 자동 실행 값이 지워짐' $null (Get-RunValue)
    Test-Value 'uninstall.ps1: 떠 있는 ClaudeBoard.exe' 0 (@(Get-BoardProcesses).Count)
    Test-Value 'uninstall.ps1: notifier 에서 지워야 할 것 중 남은 것' '' (@(Get-NotifierLeftovers) -join ', ')
    foreach ($n in $CbCmdFiles) {
      Test-Value "uninstall.ps1: 명령 $n 이 지워짐" $false (Test-Path -LiteralPath (Join-Path $CbLocalBin $n))
    }
    Test-Value 'uninstall.ps1: Claude Code 로그인(.credentials.json)은 그대로' $CbBaselineCredentials (Read-TextShared $CbCredentials)
  } -Cleanup {
    Reset-Machine
  }
} finally {
  Stop-AllFakeClaude
  Stop-ClaudeBoardProcesses
  Write-CheckSummary -Title '설치 확인' -FileName 'install-checks.md'
  Stop-Transcript | Out-Null
}
Complete-Checks -Title '설치 확인'
$global:LASTEXITCODE = 0
