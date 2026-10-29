# 격리한 Claude Board 를 띄운다 — GUI 시험용(windows/ci/gui). 사용자 홈·보드·진짜 claude 를 건드리지 않는 것이 목적이다.
# 빌드한 ClaudeBoard.exe 를 새 프로세스로 띄우고, 그 프로세스가 자기 CDP 포트로 답하는지 확인한 뒤 pid 를 출력한다.
# PowerShell 5.1 과 7 모두에서 돈다.
#
#   pwsh -File launch-board.ps1 -Root <시험 폴더(절대 경로)> [-Exe <ClaudeBoard.exe>] [-Port <포트>] [-AllowOtherBoards]
#   pwsh -File launch-board.ps1 -Root <시험 폴더> -Stop          (이 스크립트가 띄워 기록해 둔 보드만 끈다)
#
# 격리(보드 프로세스의 환경변수로만 준다. 이 스크립트가 끝나도 셸에 남지 않는다)
#   CLAUDE_BOARD_HOME                       <Root>\home        — .claude·.claude.json 이 모두 이 아래로 간다
#   WEBVIEW2_USER_DATA_FOLDER               <Root>\wv2
#   WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS   --remote-debugging-port=<빈 포트>
#   CLAUDE_BOARD_CLAUDE                     <Root>\fake\fake-bin\claude.exe|<Root>\fake\fake-claude-tty.cjs   (D5, 모두 절대 경로)
#   CLAUDE_BOARD_NOTIFIER_DIR·CLAUDE_CONFIG_DIR 는 물려받았어도 지운다
# 보드 탭의 셸은 보드의 환경을 물려받지 않는다. 그래서 가짜 claude 는 자기 옆 fake-claude.env.json(D6)에서 기록 위치 따위를 읽는다.
#   <Root>\fake\fake-claude.env.json  CLAUDE_BOARD_HOME · FAKE_CLAUDE_KEYLOG(<Root>\logs\keys.log) · FAKE_CLAUDE_ARGVLOG(<Root>\logs\argv.log) + -FakeEnv 로 더한 것
#
# 확인
#   1. 이미 떠 있는 ClaudeBoard.exe 가 있으면 멈춘다(단일 인스턴스 플러그인이 새 실행을 삼키므로, 그 보드가 사용자 것일 수 있다). 절대 끄지 않는다
#   2. 띄운 새 프로세스가 -Exe 의 경로이고 살아 있는가, 그 포트를 쥔 프로세스가 그 자손인가, /json 이 답하는가
#   3. 보드 화면에서 test_claude_prefix 를 불러 위 CLAUDE_BOARD_CLAUDE 값과 같은지 본다 — 다르면 우리가 띄운 보드만 끄고 5 로 끝낸다
#
# 그 밖의 인자: -FakeScript(가짜 claude 스크립트, 기본 ..\fake-claude-tty.cjs) · -Node(기본 PATH 의 node) · -FakeEnv(@{이름='값'}, 환경 파일에 더함) ·
#   -TimeoutSec(기본 40) · -ExeArgs·-HiddenWindow(시험용: node 사본을 보드 대역으로 띄울 때. 진짜 보드에는 쓰지 않는다)
# 진행 줄 `LAUNCH|STEP|…` 을 내고, 성공하면 `BOARD_PID=<pid>` 와 `CDP_PORT=<포트>` 가 마지막 줄들이다. 띄운 보드는 <Root>\board.json 에 기록한다(-Stop·tripwire 가 쓴다).
#
# 종료코드  0 성공(마지막 줄들에 BOARD_PID=·CDP_PORT=) · 2 사용법(Root 가 상대 경로이거나 사용자 홈·.claude 안) ·
#          3 거절(다른 ClaudeBoard.exe 가 떠 있음, -Stop 이 기록한 보드와 다름) ·
#          4 띄웠으나 우리 것이 아님(바로 끝남=단일 인스턴스에 삼켜짐, 포트가 남의 것, 시간 초과) · 5 test_claude_prefix 가 기대와 다름 · 6 exe·node·가짜 스크립트 없음
param(
  [Parameter(Mandatory = $true)][string]$Root,
  [string]$Exe,
  [string[]]$ExeArgs = @(),
  [string]$FakeScript,
  [string]$Node,
  [hashtable]$FakeEnv = @{},
  [int]$Port = 0,
  [int]$TimeoutSec = 40,
  [switch]$AllowOtherBoards,
  [switch]$HiddenWindow,
  [switch]$Stop
)

$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = New-Object Text.UTF8Encoding($false)

function Stop-Script([int]$Code, [string]$Message) {
  Write-Output $Message
  exit $Code
}

function Get-Proc([int]$ProcessId) {
  return Get-CimInstance Win32_Process -Filter ("ProcessId=" + $ProcessId) -ErrorAction SilentlyContinue
}

function Same-Path([string]$A, [string]$B) {
  if (-not $A -or -not $B) { return $false }
  return [string]::Equals([IO.Path]::GetFullPath($A), [IO.Path]::GetFullPath($B), [StringComparison]::OrdinalIgnoreCase)
}

function Under-Dir([string]$Path, [string]$Dir) {
  $dirFull = [IO.Path]::GetFullPath($Dir).TrimEnd('\') + '\'
  return ([IO.Path]::GetFullPath($Path) + '\').StartsWith($dirFull, [StringComparison]::OrdinalIgnoreCase)
}

# pid 의 자손인가 — 부모 사슬을 거슬러 올라간다(부모가 자식보다 늦게 만들어졌으면 pid 재사용이라 끊는다)
function Test-Descendant([int]$ProcessId, [int]$Ancestor) {
  $seen = @{}
  $node = Get-Proc $ProcessId
  while ($node -and -not $seen.ContainsKey([int]$node.ProcessId)) {
    $seen[[int]$node.ProcessId] = $true
    if ([int]$node.ParentProcessId -eq $Ancestor) { return $true }
    $parent = Get-Proc ([int]$node.ParentProcessId)
    if (-not $parent -or ($parent.CreationDate -and $node.CreationDate -and $parent.CreationDate -gt $node.CreationDate)) { return $false }
    $node = $parent
  }
  return $false
}

# 포트를 LISTEN 하는 프로세스 pid — netstat 로 읽는다(모듈 없이, 지역 상태 단어에 기대지 않고 원격 주소 0:0 으로 가른다)
function Get-Listener([int]$ListenPort) {
  foreach ($line in (& netstat.exe -ano -p TCP)) {
    if ($line -match ('^\s*TCP\s+\S+:' + $ListenPort + '\s+(0\.0\.0\.0:0|\[::\]:0)\s+\S+\s+(\d+)\s*$')) { return [int]$Matches[2] }
  }
  return 0
}

function Get-FreePort {
  $listener = New-Object Net.Sockets.TcpListener([Net.IPAddress]::Loopback, 0)
  $listener.Start()
  $free = $listener.LocalEndpoint.Port
  $listener.Stop()
  return $free
}

function Write-Utf8([string]$Path, [string]$Text) {
  [IO.File]::WriteAllText($Path, $Text, (New-Object Text.UTF8Encoding($false)))
}

function Stop-OwnTree([int]$ProcessId) {
  $ErrorActionPreference = 'Continue'   # 5.1 은 하위 프로그램의 표준 오류 줄을 오류로 올려 멈춘다
  & taskkill.exe /PID $ProcessId /T /F 2>&1 | Out-Null
}

# ── Root 검사 ────────────────────────────────────────────────────────────────
if (-not [IO.Path]::IsPathRooted($Root) -or $Root -notmatch '^[A-Za-z]:\\') { Stop-Script 2 "LAUNCH|USAGE|-Root 는 절대 경로여야 한다: $Root" }
$Root = [IO.Path]::GetFullPath($Root).TrimEnd('\')
$userHome = [IO.Path]::GetFullPath($env:USERPROFILE).TrimEnd('\')
$docs = [Environment]::GetFolderPath('MyDocuments')
if (Same-Path $Root $userHome) { Stop-Script 2 "LAUNCH|USAGE|-Root 가 사용자 홈이다: $Root" }
foreach ($protectedTree in @((Join-Path $userHome '.claude'), (Join-Path $docs 'PowerShell'), (Join-Path $docs 'WindowsPowerShell'))) {
  if (Under-Dir $Root $protectedTree) { Stop-Script 2 "LAUNCH|USAGE|-Root 가 사용자 환경 안이다: $Root" }
}
if ($Root -match 'aijinet-project') { Stop-Script 2 "LAUNCH|USAGE|-Root 에 회사 경로 조각이 있다: $Root" }

$recordFile = Join-Path $Root 'board.json'

# ── -Stop: 이 스크립트가 띄운 보드만 ─────────────────────────────────────────────
if ($Stop) {
  if (-not (Test-Path -LiteralPath $recordFile)) { Stop-Script 3 "LAUNCH|REFUSED|기록(board.json)이 없다 — 끄지 않는다" }
  $rec = Get-Content -LiteralPath $recordFile -Raw -Encoding UTF8 | ConvertFrom-Json
  $live = Get-Proc ([int]$rec.pid)
  if (-not $live) { Stop-Script 0 "LAUNCH|STOP|이미 끝났다(pid $($rec.pid))" }
  $sameStart = $live.CreationDate -and ([datetime]$rec.startTime).ToUniversalTime().Ticks -eq $live.CreationDate.ToUniversalTime().Ticks
  if (-not $sameStart -or -not (Same-Path $live.ExecutablePath $rec.exe)) {
    Stop-Script 3 "LAUNCH|REFUSED|pid $($rec.pid) 는 기록한 보드가 아니다(시작 시각·경로가 다르다). 아무것도 끄지 않았다"
  }
  Stop-OwnTree ([int]$rec.pid)
  Stop-Script 0 "LAUNCH|STOP|보드 $($rec.pid) 와 그 자손을 껐다"
}

# ── 입력 확인 ────────────────────────────────────────────────────────────────
if (-not $Exe) { $Exe = Join-Path $PSScriptRoot '..\..\app\src-tauri\target\release\ClaudeBoard.exe' }
if (-not (Test-Path -LiteralPath $Exe -PathType Leaf)) { Stop-Script 6 "LAUNCH|MISSING|ClaudeBoard.exe 가 없다: $Exe" }
$Exe = (Resolve-Path -LiteralPath $Exe).Path
if (-not $FakeScript) { $FakeScript = Join-Path $PSScriptRoot '..\fake-claude-tty.cjs' }
if (-not (Test-Path -LiteralPath $FakeScript -PathType Leaf)) { Stop-Script 6 "LAUNCH|MISSING|가짜 claude 스크립트가 없다: $FakeScript" }
$FakeScript = (Resolve-Path -LiteralPath $FakeScript).Path
if (-not $Node) {
  $cmd = Get-Command node -ErrorAction SilentlyContinue
  if ($cmd) { $Node = $cmd.Source }
}
if (-not $Node -or -not (Test-Path -LiteralPath $Node -PathType Leaf)) { Stop-Script 6 'LAUNCH|MISSING|node 를 찾지 못했다(가짜 claude 와 CDP 확인에 쓴다)' }
$Node = (Resolve-Path -LiteralPath $Node).Path
$cdpTool = Join-Path $PSScriptRoot 'cdp.mjs'

# ── 1. 다른 보드가 떠 있으면 멈춘다 ──────────────────────────────────────────────
$others = @(Get-CimInstance Win32_Process -Filter "Name='ClaudeBoard.exe'" -ErrorAction SilentlyContinue)
if ($others.Count -gt 0 -and -not $AllowOtherBoards) {
  foreach ($o in $others) { Write-Output ("LAUNCH|RUNNING|{0}|{1}" -f $o.ProcessId, $o.ExecutablePath) }
  Stop-Script 3 'LAUNCH|REFUSED|ClaudeBoard.exe 가 이미 떠 있다. 단일 인스턴스 플러그인이 새 실행을 삼키고 그 보드는 사용자 것일 수 있어 띄우지 않는다. 아무것도 끄지 않았다. 사용자에게 알려 그 보드를 직접 끄게 한다'
}

# ── 준비: 폴더, 가짜 claude, 환경 파일 ──────────────────────────────────────────
$homeDir = Join-Path $Root 'home'
$wvDir = Join-Path $Root 'wv2'
$logDir = Join-Path $Root 'logs'
$fakeDir = Join-Path $Root 'fake'
$fakeBin = Join-Path $fakeDir 'fake-bin'
foreach ($d in @($Root, (Join-Path $homeDir '.claude'), $wvDir, $logDir, $fakeBin)) { New-Item -ItemType Directory -Force -Path $d | Out-Null }
$fakeExe = Join-Path $fakeBin 'claude.exe'
if (-not (Test-Path -LiteralPath $fakeExe) -or (Get-Item -LiteralPath $fakeExe).Length -ne (Get-Item -LiteralPath $Node).Length) {
  Copy-Item -LiteralPath $Node -Destination $fakeExe -Force
}
$fakeCjs = Join-Path $fakeDir 'fake-claude-tty.cjs'
Copy-Item -LiteralPath $FakeScript -Destination $fakeCjs -Force
$envFile = [ordered]@{
  CLAUDE_BOARD_HOME  = $homeDir
  FAKE_CLAUDE_KEYLOG = (Join-Path $logDir 'keys.log')
  FAKE_CLAUDE_ARGVLOG = (Join-Path $logDir 'argv.log')
}
foreach ($k in $FakeEnv.Keys) { $envFile[[string]$k] = [string]$FakeEnv[$k] }
Write-Utf8 (Join-Path $fakeDir 'fake-claude.env.json') ($envFile | ConvertTo-Json)
$prefix = $fakeExe + '|' + $fakeCjs

# ── 띄운다 ───────────────────────────────────────────────────────────────────
if ($Port -le 0) { $Port = Get-FreePort }
# ShellExecute 로 띄운다 — 이 셸의 표준 입출력 핸들을 보드가 물려받지 않는다(파이프로 이 스크립트를 부른 쪽이 보드가 끝날 때까지 기다리는 일이 없다).
# ShellExecute 는 프로세스 환경을 이 셸에서 가져가므로, 필요한 변수만 잠깐 정하고 띄운 직후 원래대로 돌려 놓는다.
$psi = New-Object Diagnostics.ProcessStartInfo
$psi.FileName = $Exe
$psi.Arguments = (@($ExeArgs | ForEach-Object { '"' + $_ + '"' }) -join ' ')
$psi.WorkingDirectory = $Root
$psi.UseShellExecute = $true
if ($HiddenWindow) { $psi.WindowStyle = [Diagnostics.ProcessWindowStyle]::Hidden }
$wanted = [ordered]@{
  CLAUDE_BOARD_HOME                     = $homeDir
  WEBVIEW2_USER_DATA_FOLDER             = $wvDir
  WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=$Port"
  CLAUDE_BOARD_CLAUDE                   = $prefix
  CLAUDE_BOARD_NOTIFIER_DIR             = $null
  CLAUDE_CONFIG_DIR                     = $null
}
$savedEnv = @{}
foreach ($name in $wanted.Keys) {
  $savedEnv[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
  $value = $wanted[$name]
  if ($null -eq $value) { $value = [NullString]::Value }   # $null 은 빈 문자열로 바뀌어 "빈 값으로 있는" 변수가 된다 — 지우려면 NullString 이다
  [Environment]::SetEnvironmentVariable($name, $value, 'Process')
}
try {
  $proc = [Diagnostics.Process]::Start($psi)
} finally {
  foreach ($name in $savedEnv.Keys) {
    $back = $savedEnv[$name]
    if ($null -eq $back) { $back = [NullString]::Value }
    [Environment]::SetEnvironmentVariable($name, $back, 'Process')
  }
}
$boardPid = $proc.Id
Write-Output "LAUNCH|STEP|started pid=$boardPid port=$Port"

# ── 2. 우리 것인지 확인 ──────────────────────────────────────────────────────
$deadline = (Get-Date).AddSeconds($TimeoutSec)
$info = $null
$answered = $false
while ((Get-Date) -lt $deadline) {
  if ($proc.HasExited) {
    Stop-Script 4 "LAUNCH|SWALLOWED|새 프로세스 $boardPid 가 바로 끝났다(종료코드 $($proc.ExitCode)). 다른 인스턴스에 삼켜졌을 가능성이 크다. 다른 ClaudeBoard.exe(사용자 보드)는 건드리지 않았다"
  }
  $info = Get-Proc $boardPid
  if ($info -and (Same-Path $info.ExecutablePath $Exe)) {
    try {
      $json = (Invoke-WebRequest -UseBasicParsing -Uri "http://127.0.0.1:$Port/json" -TimeoutSec 3).Content
      if (@($json | ConvertFrom-Json | Where-Object { $_.type -eq 'page' }).Count -gt 0) { $answered = $true; break }
    } catch { }
  }
  Start-Sleep -Milliseconds 300
}
if (-not $answered) {
  Stop-OwnTree $boardPid
  Stop-Script 4 "LAUNCH|TIMEOUT|$TimeoutSec 초 안에 새 프로세스 $boardPid 의 포트 $Port 가 답하지 않았다. 우리가 띄운 것만 껐다"
}
if (-not (Same-Path $info.ExecutablePath $Exe)) {
  Stop-OwnTree $boardPid
  Stop-Script 4 "LAUNCH|NOT-OURS|pid $boardPid 의 경로가 다르다: $($info.ExecutablePath)"
}
Write-Output "LAUNCH|STEP|answered"
$listener = Get-Listener $Port
Write-Output "LAUNCH|STEP|listener=$listener"
if ($listener -le 0 -or ($listener -ne $boardPid -and -not (Test-Descendant $listener $boardPid))) {
  Stop-OwnTree $boardPid
  Stop-Script 4 "LAUNCH|NOT-OURS|포트 $Port 를 쥔 프로세스($listener)가 새 보드 $boardPid 의 자손이 아니다. 우리가 띄운 것만 껐다"
}

# 기록 — -Stop 과 tripwire 가 읽는다
$record = [pscustomobject]@{ pid = $boardPid; startTime = $info.CreationDate.ToUniversalTime().ToString('o'); exe = $Exe; port = $Port; root = $Root; home = $homeDir; fakeDir = $fakeDir; prefix = $prefix }
Write-Utf8 $recordFile ($record | ConvertTo-Json)

# ── 3. 보드 화면이 아는 시험용 claude 값이 기대와 같은가 ──────────────────────────
$prefixOk = $false
$seen = ''
$checkDeadline = (Get-Date).AddSeconds([Math]::Max(10, [int](($deadline - (Get-Date)).TotalSeconds)))
while ((Get-Date) -lt $checkDeadline -and -not $prefixOk) {
  $ErrorActionPreference = 'Continue'   # 5.1: cdp.mjs 가 표준 오류로 실패를 알려도 여기서 멈추지 않고 종료코드를 본다
  $out = & $Node $cdpTool --port $Port eval "window.__TAURI_INTERNALS__.invoke('test_claude_prefix')" 2>&1
  $ErrorActionPreference = 'Stop'
  if ($LASTEXITCODE -eq 0) {
    $seen = ($out | ForEach-Object { "$_" }) -join "`n"
    try {
      $value = $seen | ConvertFrom-Json
      if ($null -ne $value -and (@($value) -join '|') -ieq $prefix) { $prefixOk = $true }
      else { break }   # 답은 왔는데 값이 다르다 — 기다려도 바뀌지 않는다
    } catch { break }
  } else {
    Start-Sleep -Milliseconds 400
  }
}
if (-not $prefixOk) {
  Stop-OwnTree $boardPid
  Stop-Script 5 "LAUNCH|PREFIX-MISMATCH|test_claude_prefix 가 기대($prefix)와 다르다: $seen. 진짜 claude 가 쳐질 수 있어 우리가 띄운 보드를 껐다"
}

Write-Output "LAUNCH|OK|board=$boardPid|port=$Port"
Write-Output "ROOT=$Root"
Write-Output "HOME=$homeDir"
Write-Output "FAKE_PREFIX=$prefix"
Write-Output "CDP_PORT=$Port"
Write-Output "BOARD_PID=$boardPid"
exit 0
