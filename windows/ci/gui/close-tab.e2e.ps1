# close-tab e2e — 진짜 보드와 명령줄 도구(--close-tab)를 끝까지 잇는 GUI 시험(windows/ci/gui).
# launch-board.ps1 이 띄운 격리 보드에 ClaudeBoard.exe --close-tab 요청을 직접 날리고,
# 돌고 있는 claude 를 그냥 닫으려는 b · 끝내고 닫는 c · 닫힌 뒤의 d · 겨냥 없는 e · 보드가 꺼졌을 때의 f 를
# 차례로 본 뒤 모두 정리한다. open-tab.e2e.ps1 의 뼈대와 같은 모양새다.
# PowerShell 5.1 과 7 모두에서 돈다. 진행 줄은 `E2E|STEP|…`, 판정 줄은 `E2E|PASS|<이름>` / `E2E|FAIL|<이름>|<이유>` 다.
#
#   pwsh -File close-tab.e2e.ps1 [-Root <시험 폴더(절대 경로)>] [-Exe <ClaudeBoard.exe>]
#   -Root 를 주지 않으면 $env:TEMP 아래에 새 시험 폴더를 만든다. 기본 -Exe 는 launch-board.ps1 과 같다
#
# 검사 — 명령줄 도구의 답은 한 줄 JSON(ok·status·tab·session·pid·why, close.rs 의 CloseTabReply)이고,
#        답이 ok 면 0, 아니면 1 로 끝난다
#   a  --open-tab --cwd <Root>\work -- claude --name e2e-close   코드 0 · status=opened · argv.log 가 e2e-close 줄을 얻는다(15 초 안)
#   b  --close-tab --title e2e-close(claude 가 돌 때)            코드 1 · status=busy · 이유가 'claude 가 돌고' 다
#                                                                 (탭이 막 뜨는 중·표지가 안 온 중의 busy·refused 는 기다렸다 다시 본다)
#   c  같은 요청에 --stop                                        코드 0 · status=closed(/exit 로 가짜 claude 를 끝낸 뒤 닫는다)
#   d  b 의 요청을 다시                                           코드 1 · status=not-found(닫혀 있으므로)
#   e  --close-tab(겨냥 없이)                                    코드 1 · status=bad-args(--session 이나 --title 가운데 하나가 필요)
#   f  launch-board -Stop 뒤 같은 요청                            코드 1 · status=board-off(닫으려고 보드를 켜지 않는다) ·
#                                                                 보드가 꺼진 뒤 같은 exe 경로의 새 ClaudeBoard.exe 가 뜨지 않는다
#
# 보드와 명령줄 도구가 같은 요청 폴더를 쓰게 하는 장치 — 양쪽 모두 CLAUDE_BOARD_HOME 을 쥐어 준다.
# paths.rs 의 local_app_data 가 이 홈을 `<CLAUDE_BOARD_HOME>\AppData\Local` 로 바꾸므로 닫기 요청 폴더가 시험 홈 아래로 간다.
#
# 종료코드  0 전부 통과 · 1 판정 실패(검사·철선) · 3 거절(다른 보드가 떠 있다) ·
#          2·4·5·6 은 launch-board 종료코드를 그대로 물려준다(사용법·우리 보드 아님·접두사 다름·exe 없음)
param(
  [string]$Root,
  [string]$Exe
)

$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = New-Object Text.UTF8Encoding($false)

$launchScript = Join-Path $PSScriptRoot 'launch-board.ps1'
$tripwireScript = Join-Path $PSScriptRoot 'tripwire.ps1'
# 시험 스크립트는 자식 프로세스로 부른다 — exit 종료코드·환경변수·EAP 바꿈이 이쪽 흐름에 묻지 않게(tripwire selftest 의 셸 뽑기와 같다)
$shellExe = (Get-Process -Id $PID).Path

$script:failCount = 0
$script:rootDir = ''
$script:homeDir = ''
$script:boardExe = ''
$script:fakePrefix = ''

function Write-Step([string]$Text) {
  Write-Output ('E2E|STEP|' + $Text)
}

# 판정 한 줄 — 실패만 셈에 센다(마지막 종료코드가 답이다)
function Write-Check([string]$Name, [bool]$Ok, [string]$Why) {
  if ($Ok) {
    Write-Output ('E2E|PASS|' + $Name)
    return
  }
  $script:failCount += 1
  Write-Output ('E2E|FAIL|' + $Name + '|' + $Why)
}

# 자식 프로세스로 시험 스크립트 한 편을 돌린다 — 종료코드와 출력 줄들을 돌려준다
function Invoke-ChildScript([string]$ScriptPath, [hashtable]$NamedArgs) {
  $ErrorActionPreference = 'Continue'   # 5.1 은 자식의 표준 오류 줄을 오류로 올려 멈춘다
  $out = & $shellExe -NoProfile -ExecutionPolicy Bypass -File $ScriptPath @NamedArgs 2>&1
  $ErrorActionPreference = 'Stop'
  return [pscustomobject]@{ code = $LASTEXITCODE; lines = @($out | ForEach-Object { "$_" }) }
}

# 경로 같은지 견주기 — 대소문자를 묻지 않는다(launch-board.ps1 의 Same-Path 와 같은 본뜸)
function Same-Path([string]$A, [string]$B) {
  if (-not $A -or -not $B) { return $false }
  return [string]::Equals([IO.Path]::GetFullPath($A), [IO.Path]::GetFullPath($B), [StringComparison]::OrdinalIgnoreCase)
}

# 답 판정 — 종료코드·status 를 견주고 어긋나면 (ok, why) 쌍으로 이유를 꾸며 돌려준다
function Test-Reply($Result, [int]$WantCode, [string]$WantStatus) {
  if ($null -eq $Result.reply) {
    return @{ ok = $false; why = ('답(JSON)을 읽지 못했다 — 종료코드 ' + $Result.code + ', 출력 마지막 줄: ' + $Result.replyLine) }
  }
  $why = ''
  if ($Result.code -ne $WantCode) {
    $why = ('종료코드 ' + $Result.code + ' 은 기대 ' + $WantCode + ' 이 아니다')
  }
  $gotStatus = [string]$Result.reply.status
  if ($gotStatus -ne $WantStatus) {
    if ($why) { $why += ' / ' }
    $why += ('status ' + $gotStatus + ' 은 기대 ' + $WantStatus + ' 이 아니다')
    if ($Result.reply.why) { $why += (' — 도구 이유: ' + [string]$Result.reply.why) }
  }
  if ($why) { return @{ ok = $false; why = $why } }
  return @{ ok = $true; why = '' }
}

# ── 명령줄 도구 한 번 부르기 ─────────────────────────────────────────────────────────
# ClaudeBoard.exe --open-tab / --close-tab … 을 셸 없이 띄워 답(한 줄 JSON)과 종료코드를 돌려준다.
# 보드와 같은 CLAUDE_BOARD_HOME 을 이 환경에 쥐어 주는 것이 핵심이다 — 열기·닫기 요청 폴더가 그 아래로 간다(paths.rs local_app_data).
function Invoke-BoardTool([string[]]$ToolArgs) {
  $tool = New-Object Diagnostics.ProcessStartInfo
  $tool.FileName = $script:boardExe
  $tool.Arguments = ($ToolArgs | ForEach-Object { '"' + $_ + '"' }) -join ' '
  $tool.WorkingDirectory = $script:rootDir
  $tool.UseShellExecute = $false
  $tool.CreateNoWindow = $true
  $tool.RedirectStandardOutput = $true
  $tool.RedirectStandardError = $true
  # 도구와 답 줄은 UTF-8 — 5.1 리더의 기본 코드페이지로 한글(why)이 깨지지 않게 리더도 UTF-8 로 묶는다
  $tool.StandardOutputEncoding = New-Object Text.UTF8Encoding($false)
  $tool.StandardErrorEncoding = New-Object Text.UTF8Encoding($false)
  $toolEnv = $tool.EnvironmentVariables
  $toolEnv['CLAUDE_BOARD_HOME'] = $script:homeDir
  $toolEnv['CLAUDE_BOARD_CLAUDE'] = $script:fakePrefix
  $toolEnv['WEBVIEW2_USER_DATA_FOLDER'] = (Join-Path $script:rootDir 'wv2')
  foreach ($unwanted in @('CLAUDE_CONFIG_DIR', 'CLAUDE_BOARD_NOTIFIER_DIR')) {
    [void]$toolEnv.Remove($unwanted)
  }

  $process = [Diagnostics.Process]::Start($tool)
  # 표준 오류는 비동기로 읽는다 — 두 파이프를 차례로 다 읽으면 버퍼가 찼을 때 교착된다
  $errTask = $process.StandardError.ReadToEndAsync()
  if (-not $process.WaitForExit(75000)) {
    # 도구는 스스로 30 초에 기다림을 끝내지만, 몸이 죽었을 때를 위해 넉넉한 상한 하나를 둔다
    $process.Kill()
    $process.WaitForExit()
    return [pscustomobject]@{ code = -1; out = ''; err = '75 초 넘게 답이 없는 도구다 — 이쪽에서 끝냈다'; reply = $null; replyLine = '' }
  }
  $out = $process.StandardOutput.ReadToEnd()
  $err = $errTask.GetAwaiter().GetResult()

  # 답 줄 고르기 — 뒤에서부터 읽어 status 칸이 있는 JSON 줄 하나를 답으로 본다(다른 줄이 섞여도 견딘다)
  $reply = $null
  $replyLine = ''
  $lines = @($out -split "`r?`n" | Where-Object { $_.Trim() })
  for ($i = $lines.Count - 1; $i -ge 0; $i--) {
    try {
      $candidate = $lines[$i] | ConvertFrom-Json
      if ($candidate -and $candidate.PSObject.Properties['status']) {
        $reply = $candidate
        $replyLine = $lines[$i]
        break
      }
    } catch { }
  }
  return [pscustomobject]@{ code = $process.ExitCode; out = $out; err = $err; reply = $reply; replyLine = $replyLine }
}

# ── 시험 폴더·입력 준비 ──────────────────────────────────────────────────────────────
if (-not $Exe) {
  $Exe = Join-Path $PSScriptRoot '..\..\app\src-tauri\target\release\ClaudeBoard.exe'   # launch-board.ps1 의 기본과 같다
}
if (-not $Root) {
  $Root = Join-Path ([IO.Path]::GetTempPath()) ('close-tab-e2e-' + [guid]::NewGuid().ToString('N'))
} elseif (-not [IO.Path]::IsPathRooted($Root)) {
  Write-Output ('E2E|FAIL|usage|-Root 는 절대 경로여야 한다: ' + $Root)
  exit 2
}
$Root = [IO.Path]::GetFullPath($Root).TrimEnd('\')
New-Item -ItemType Directory -Force -Path $Root | Out-Null
$stateFile = Join-Path $Root 'tripwire-state.json'

Write-Step ('root=' + $Root)
Write-Step ('exe=' + $Exe)

# ── 1. 철선 before → 보드 띄우기 → 기록 읽기 ────────────────────────────────────────
$tripBefore = Invoke-ChildScript $tripwireScript @{ Phase = 'before'; State = $stateFile }
if ($tripBefore.code -ne 0) {
  Write-Check 'tripwire-before' $false ('종료코드 ' + $tripBefore.code + ': ' + ($tripBefore.lines -join ' | '))
  exit 1
}
Write-Output $tripBefore.lines

$launchArgs = @{ Root = $Root }
if ($Exe) { $launchArgs['Exe'] = $Exe }
$launch = Invoke-ChildScript $launchScript $launchArgs
if ($launch.code -ne 0) {
  # launch-board 의 종료코드를 물려준다 — 3 은 거절(다른 보드가 떠 있다), 2 사용법·4 우리 보드 아님·5 접두사 다름·6 exe 없음
  Write-Check 'launch-board' $false ('종료코드 ' + $launch.code + ': ' + ($launch.lines -join ' | '))
  exit $launch.code
}
Write-Output $launch.lines

# launch-board 가 기록한 격리 값들이 시험의 재료다 — exe 는 Resolve-Path 된 절대 경로다
$boardRec = Get-Content (Join-Path $Root 'board.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$script:rootDir = $boardRec.root
$script:homeDir = $boardRec.home
$script:boardExe = $boardRec.exe
$script:fakePrefix = $boardRec.prefix
$argvLog = Join-Path $script:rootDir 'logs\argv.log'
$workDir = Join-Path $script:rootDir 'work'
New-Item -ItemType Directory -Force -Path $workDir | Out-Null

# ── 검사 a — 새 탭이 열리고 가짜 claude 가 argv.log 를 남긴다 ────────────────────────
$argvBefore = 0
if (Test-Path -LiteralPath $argvLog) { $argvBefore = @([IO.File]::ReadAllLines($argvLog)).Count }
$opened = Invoke-BoardTool @('--open-tab', '--cwd', $workDir, '--', 'claude', '--name', 'e2e-close')
$openedJudge = Test-Reply $opened 0 'opened'
Write-Check 'a-opened' $openedJudge.ok $openedJudge.why

# 가짜 claude 명령줄이 argv.log 에 쌓였는가 — 탭이 띄워지는 데 몇 초 걸린다(15 초 안에 새 줄)
$argvOk = $false
$argvDeadline = (Get-Date).AddSeconds(15)
while ((Get-Date) -lt $argvDeadline) {
  $argvLines = @()
  try { $argvLines = [IO.File]::ReadAllLines($argvLog) } catch { }
  for ($i = $argvBefore; $i -lt $argvLines.Count; $i++) {
    if ($argvLines[$i].Contains('e2e-close')) {
      $argvOk = $true
      break
    }
  }
  if ($argvOk) { break }
  Start-Sleep -Milliseconds 300
}
$argvWhy = '15 초 안에 argv.log 에 e2e-close 줄이 없다 — 탭의 가짜 claude 가 띄워지지 않았다'
if ($argvOk) { $argvWhy = '' }
Write-Check 'a-argvlog' $argvOk $argvWhy

# ── 검사 b — claude 가 돌고 있는 탭은 그냥은 닫지 않는다(busy) ────────────────────────
# 탭이 막 뜨는 동안(프론트의 막 뜸 표지·닫기 표지가 프론트에서 오기 전)의 busy('막 뜨는 중')·refused('바깥 열기')는
# 기다렸다 다시 본다. claude 가 그려지기 전의 프롬프트 탭이라면 'closed' 로 닫혀버리니, 이 때는 탭을 다시 열어 기다린다.
# 판정 — 종료코드 1 · status busy · 이유가 'claude 가 돌고' 로 시작하는 답이 오면 통과다.
$busyOk = $false
$busyWhy = '60 초 안에 claude 가 돈다는 busy 답을 얻지 못했다'
$busyDeadline = (Get-Date).AddSeconds(60)
while ((Get-Date) -lt $busyDeadline) {
  $hit = Invoke-BoardTool @('--close-tab', '--title', 'e2e-close')
  $gotStatus = [string]$hit.reply.status
  $gotWhy = [string]$hit.reply.why
  $busyWhy = ('기대 busy(claude 가 돌고)를 못 얻었다 — 마지막 답: 종료코드 ' + $hit.code + ', status ' + $gotStatus + ', 이유: ' + $gotWhy)
  if (($hit.code -eq 1) -and ($gotStatus -eq 'busy') -and ($gotWhy -like '*claude 가 돌고*')) {
    $busyOk = $true
    break
  }
  if (($hit.code -eq 0) -and ($gotStatus -eq 'closed')) {
    Write-Step 'b — claude 가 그려지기 전의 프롬프트 탭으로 닫혔다. 탭을 다시 열어 기다린다'
    [void](Invoke-BoardTool @('--open-tab', '--cwd', $workDir, '--', 'claude', '--name', 'e2e-close'))
    Start-Sleep -Seconds 3
  } else {
    Start-Sleep -Seconds 2
  }
}
Write-Check 'b-busy' $busyOk $busyWhy

# ── 검사 c — --stop : /exit 로 끝낸 뒤 닫는다(closed) ────────────────────────────────
# 잠자는 틈에 busy 답이 다시 오면 기다렸다 본다(탭은 남아 있으니). 탭이 없어진 not-found, 그런데 closed 도
# 아닌 ok 답은 기다려도 같으니 바로 어긋난 판정이다.
$stopCloseOk = $false
$stopCloseWhy = '60 초 안에 --stop 의 closed 답을 얻지 못했다'
$stopDeadline = (Get-Date).AddSeconds(60)
while ((Get-Date) -lt $stopDeadline) {
  $hit = Invoke-BoardTool @('--close-tab', '--title', 'e2e-close', '--stop')
  $gotStatus = [string]$hit.reply.status
  $gotWhy = [string]$hit.reply.why
  $stopCloseWhy = ('마지막 답: 종료코드 ' + $hit.code + ', status ' + $gotStatus + ', 이유: ' + $gotWhy)
  if (($hit.code -eq 0) -and ($gotStatus -eq 'closed')) {
    $stopCloseOk = $true
    break
  }
  if ($hit.code -eq 0 -or $gotStatus -eq 'not-found') { break }
  Start-Sleep -Seconds 3
}
Write-Check 'c-stop-closed' $stopCloseOk $stopCloseWhy

# ── 검사 d — 닫힌 탭을 다시 닫으려면 not-found ──────────────────────────────────────
$again = Invoke-BoardTool @('--close-tab', '--title', 'e2e-close')
$againJudge = Test-Reply $again 1 'not-found'
Write-Check 'd-not-found' $againJudge.ok $againJudge.why

# ── 검사 e — 겨냥 없으면 도구가 바로 bad-args 로 답한다 ──────────────────────────────
# 닫기 요청은 쓰지 않는다(도구 쪽 검사) — 보드를 오가는 일이 없어 잠깐의 판정이다
$aimless = Invoke-BoardTool @('--close-tab')
$aimlessJudge = Test-Reply $aimless 1 'bad-args'
Write-Check 'e-bad-args' $aimlessJudge.ok $aimlessJudge.why

# ── 검사 f — 보드가 꺼졌을 때는 켜지 않고 board-off 로 답한다 ────────────────────────
Write-Step '검사 f 시작 — 보드를 끈다'
$stopAt = (Get-Date)
$fstop = Invoke-ChildScript $launchScript @{ Root = $script:rootDir; Stop = $true }
$fstopOk = ($fstop.code -eq 0)
Write-Check 'f-stop' $fstopOk ('종료코드 ' + $fstop.code + ': ' + ($fstop.lines -join ' | '))
$fOffOk = $false
$fOffWhy = '앞의 launch-board -Stop 이 실패해 보드가 꺼진 상태를 만들지 못했다'
if ($fstopOk) {
  # 보드가 죽은 뒤 인스턴스 뮤텍스가 내려가는 잠깐을 견디게 — 기다림을 짧게(--timeout 8) 걸어 busy·timeout 답도 곧 다시 본다
  $fDeadline = (Get-Date).AddSeconds(45)
  while ((Get-Date) -lt $fDeadline) {
    $offHit = Invoke-BoardTool @('--close-tab', '--title', 'e2e-close', '--timeout', '8')
    $gotStatus = [string]$offHit.reply.status
    $gotWhy = [string]$offHit.reply.why
    $fOffWhy = ('마지막 답: 종료코드 ' + $offHit.code + ', status ' + $gotStatus + ', 이유: ' + $gotWhy)
    if (($offHit.code -eq 1) -and ($gotStatus -eq 'board-off') -and ($gotWhy -like '*보드가 꺼져*')) {
      $fOffOk = $true
      break
    }
    Start-Sleep -Seconds 3
  }
}
Write-Check 'f-board-off' $fOffOk $fOffWhy

# -Stop 순간 뒤에 뜬 우리 exe 경로의 보드가 없는가 — 닫기 도구는 닫으려고 보드를 켜지 않는다
$relaunched = @(Get-CimInstance Win32_Process -Filter "Name='ClaudeBoard.exe'" -ErrorAction SilentlyContinue | Where-Object {
  (Same-Path $_.ExecutablePath $script:boardExe) -and ($_.CreationDate -and ([datetime]$_.CreationDate -gt $stopAt))
})
$relaunchOk = ($relaunched.Count -eq 0)
$relaunchWhy = '보드가 꺼진 뒤 같은 exe 로 새 ClaudeBoard.exe 가 떴다: pid ' + ((@($relaunched | ForEach-Object { [int]$_.ProcessId })) -join ',')
if (-not $relaunchOk) {
  foreach ($stray in $relaunched) {
    $ErrorActionPreference = 'Continue'
    & taskkill.exe /PID ([int]$stray.ProcessId) /T /F 2>&1 | Out-Null
    $ErrorActionPreference = 'Stop'
  }
}
Write-Check 'f-no-relaunch' $relaunchOk $relaunchWhy

# ── 2. 철선 after — 검사 도중 새로 뜬 진짜 claude·잠긴 파일의 변화를 본다 ───────────────
# 청소 — 검사가 어긋나 보드가 살아 있으면 우리 기록(board.json)만 골라 끈다(이미 끝났다면 -Stop 은 0 이다)
$cleanup = Invoke-ChildScript $launchScript @{ Root = $script:rootDir; Stop = $true }
$cleanupWhy = ('종료코드 ' + $cleanup.code + ': ' + ($cleanup.lines -join ' | '))
if ($cleanup.code -eq 0) {
  Write-Step ('청소 — ' + ($cleanup.lines -join ' | '))
} elseif ($cleanup.code -eq 3) {
  Write-Step ('청소 — 기록한 보드를 끄지 않았다(기록 없음 또는 pid 어긋남): ' + ($cleanup.lines -join ' | '))
} else {
  Write-Check 'cleanup-stop' $false $cleanupWhy
}
Start-Sleep -Milliseconds 500   # 탭의 자손(가짜 claude)가 뒤따라 끝난 것을 철선 전에 가만히 둔다

$tripAfter = Invoke-ChildScript $tripwireScript @{ Phase = 'after'; State = $stateFile; BoardFile = (Join-Path $script:rootDir 'board.json') }
Write-Output $tripAfter.lines
if ($tripAfter.code -ne 0) {
  Write-Check 'tripwire-after' $false ('종료코드 ' + $tripAfter.code + ': 사용자 환경이 시험 중 변했거나 진짜 claude 가 새로 떴다')
}

# ── 종료 — 1 이 하나라도 실패(철선 포함), 없으면 0. launch-board 코드는 위에서 물려 이미 끝났다 ──
if ($script:failCount -gt 0) {
  Write-Step ('판실패 ' + $script:failCount + '개 — 위 FAIL 줄을 본다')
  exit 1
}
Write-Step '전부 통과'
exit 0
