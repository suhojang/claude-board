# open-tab e2e — 진짜 보드와 명령줄 도구(--open-tab)를 끝까지 잇는 GUI 시험(windows/ci/gui).
# launch-board.ps1 이 띄운 격리 보드에 ClaudeBoard.exe --open-tab 요청을 직접 날리고,
# 답(status)·가짜 claude 의 argv.log·--launch 재시작 흐름을 차례로 확인한 뒤 모두 정리한다.
# PowerShell 5.1 과 7 모두에서 돈다. 진행 줄은 `E2E|STEP|…`, 판정 줄은 `E2E|PASS|<이름>` / `E2E|FAIL|<이름>|<이유>` 다.
#
#   pwsh -File open-tab.e2e.ps1 [-Root <시험 폴더(절대 경로)>] [-Exe <ClaudeBoard.exe>]
#   -Root 를 주지 않으면 $env:TEMP 아래에 새 시험 폴더를 만든다. 기본 -Exe 는 launch-board.ps1(132 줄)과 같다
#
# 검사 — 명령줄 도구의 답은 한 줄 JSON(ok·status·tab·session·pid·why, open_tab/args.rs 의 OpenTabReply)이고,
#        답이 ok 면 0, 아니면 1 로 끝난다
#   a  --open-tab --cwd <Root>\work -- claude --name e2e-a    코드 0 · status=opened · argv.log 가 e2e-a 줄을 얻는다(15 초 안)
#   b  같은 요청에 --no-focus                                  코드 0 · status=exists(이미 떠 있는 탭을 뒤에서 연다)
#   c  --open-tab --cwd <Root>\missing -- claude --name e2e-c  코드 1 · status=bad-args(없는 폴더는 보드가 거절한다, handle.rs)
#   d  launch-board -Stop 뒤 --open-tab --launch … e2e-d        코드 0 · status=opened · -Stop 뒤에 뜬 보드만 골라 끈다
#
# 보드와 명령줄 도구가 같은 요청 폴더를 쓰게 하는 장치 — 양쪽 모두 CLAUDE_BOARD_HOME 을 쥐어 준다.
# paths.rs 의 local_app_data 가 이 홈을 `<CLAUDE_BOARD_HOME>\AppData\Local` 로 바꾸므로 요청 폴더가 시험 홈 아래로 간다.
# --launch 로 다시 띄워지는 보드도 도구의 환경을 물려받아 같은 격리를 이어간다(std::process::Command 는 부모 환경을 준다).
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
# ClaudeBoard.exe --open-tab … 을 셸 없이 띄워 답(한 줄 JSON)과 종료코드를 돌려준다.
# 보드와 같은 CLAUDE_BOARD_HOME 을 이 환경에 쥐어 주는 것이 핵심이다 — 요청 폴더가 그 아래로 간다(paths.rs local_app_data).
function Invoke-OpenTab([string[]]$ToolArgs) {
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
    # 도구는 스스로 20·40 초에 기다림을 끝내지만, 몸이 죽었을 때를 위해 넉넉한 상한 하나를 둔다
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
  $Root = Join-Path ([IO.Path]::GetTempPath()) ('open-tab-e2e-' + [guid]::NewGuid().ToString('N'))
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
$missingDir = Join-Path $script:rootDir 'missing'
New-Item -ItemType Directory -Force -Path $workDir | Out-Null
if (Test-Path -LiteralPath $missingDir) { Remove-Item -LiteralPath $missingDir -Recurse -Force }   # 옮은 흔적이 c 검사를 속이지 않게

# ── 검사 a — 새 탭이 열리고 가짜 claude 가 argv.log 를 남긴다 ────────────────────────
$argvBefore = 0
if (Test-Path -LiteralPath $argvLog) { $argvBefore = @([IO.File]::ReadAllLines($argvLog)).Count }
$opened = Invoke-OpenTab @('--open-tab', '--cwd', $workDir, '--', 'claude', '--name', 'e2e-a')
$openedJudge = Test-Reply $opened 0 'opened'
Write-Check 'a-opened' $openedJudge.ok $openedJudge.why

# 가짜 claude 명령줄이 argv.log 에 쌓였는가 — 탭이 띄워지는 데 몇 초 걸린다(15 초 안에 새 줄)
$argvOk = $false
$argvDeadline = (Get-Date).AddSeconds(15)
while ((Get-Date) -lt $argvDeadline) {
  $argvLines = @()
  try { $argvLines = [IO.File]::ReadAllLines($argvLog) } catch { }
  for ($i = $argvBefore; $i -lt $argvLines.Count; $i++) {
    if ($argvLines[$i].Contains('e2e-a')) {
      $argvOk = $true
      break
    }
  }
  if ($argvOk) { break }
  Start-Sleep -Milliseconds 300
}
$argvWhy = '15 초 안에 argv.log 에 e2e-a 줄이 없다 — 탭의 가짜 claude 가 띄워지지 않았다'
if ($argvOk) { $argvWhy = '' }
Write-Check 'a-argvlog' $argvOk $argvWhy

# ── 검사 b — 같은 탭을 뒤에서 가린다(exists) ────────────────────────────────────────
$reuse = Invoke-OpenTab @('--open-tab', '--no-focus', '--cwd', $workDir, '--', 'claude', '--name', 'e2e-a')
$reuseJudge = Test-Reply $reuse 0 'exists'
Write-Check 'b' $reuseJudge.ok $reuseJudge.why

# ── 검사 c — 없는 폴더는 보드가 bad-args 로 받아 준다 ────────────────────────────────
$refused = Invoke-OpenTab @('--open-tab', '--cwd', $missingDir, '--', 'claude', '--name', 'e2e-c')
$refusedJudge = Test-Reply $refused 1 'bad-args'
$refusedWhy = $refusedJudge.why
if ($refusedJudge.ok -and ([string]$refused.reply.why -notlike '*폴더가 없습니다*')) {
  $refusedJudge.ok = $false   # bad-args 도, 그 벌지만이 폴더가 아닌 것일 때 c 의 판정이다
  $refusedWhy = 'bad-args 이지만 이유가 다르다: ' + [string]$refused.reply.why
}
Write-Check 'c' $refusedJudge.ok $refusedWhy

# ── 검사 d — 보드를 끈 뒤 --launch 로 도구가 보드를 띄운다 ───────────────────────────
Write-Step '검사 d 시작 — 기존 보드를 끈다'
$stopAt = (Get-Date)
$stop = Invoke-ChildScript $launchScript @{ Root = $script:rootDir; Stop = $true }
$stopOk = ($stop.code -eq 0)
$relaunched = $null
$killOk = $true
if ($stopOk) {
  $back = Invoke-OpenTab @('--open-tab', '--launch', '--cwd', $workDir, '--', 'claude', '--name', 'e2e-d')
} else {
  Write-Check 'd' $false ('launch-board -Stop 실패(코드 ' + $stop.code + '): ' + ($stop.lines -join ' | '))
}

# -Stop 순간 뒤에 뜬 우리 exe 보드만 골라 끈다 — 성패와 무관하게 청소한다(도구가 답을 기다리다 끝났을 수도 있다)
$relaunched = @(Get-CimInstance Win32_Process -Filter "Name='ClaudeBoard.exe'" -ErrorAction SilentlyContinue | Where-Object {
  (Same-Path $_.ExecutablePath $script:boardExe) -and ($_.CreationDate -and ([datetime]$_.CreationDate -gt $stopAt))
})
foreach ($stray in $relaunched) {
  $ErrorActionPreference = 'Continue'
  & taskkill.exe /PID ([int]$stray.ProcessId) /T /F 2>&1 | Out-Null
  $ErrorActionPreference = 'Stop'
  if ($LASTEXITCODE -ne 0) { $killOk = $false }
}
if ($relaunched.Count -gt 0) {
  $killWord = '실패'
  if ($killOk) { $killWord = '완료' }
  Write-Step ('띄워진 보드 정리(' + $killWord + ') pid=' + ((@($relaunched | ForEach-Object { [int]$_.ProcessId })) -join ','))
}
Start-Sleep -Milliseconds 500   # 탭의 자손(가짜 claude)가 뒤따라 끝난 것을 철선 전에 가만히 둔다
if ($stopOk) {
  $backJudge = Test-Reply $back 0 'opened'
  $dWhy = $backJudge.why
  $dOk = $backJudge.ok
  if ($dOk -and ($relaunched.Count -lt 1)) {
    $dOk = $false
    $dWhy = '--launch 가 띄운 보드(ClaudeBoard.exe)를 못 찾았다'
  } elseif ($dOk -and (-not $killOk)) {
    $dOk = $false
    $dWhy = '띄워진 보드를 끄지 못했다(taskkill)'
  }
  Write-Check 'd' $dOk $dWhy
}

# ── 2. 철선 after — 검사 도중 새로 뜬 진짜 claude·잠긴 파일의 변화를 본다 ───────────────
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
