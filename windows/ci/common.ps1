# Claude Board for Windows — CI 확인 스크립트의 공통 도구. install-checks.ps1·screen-checks.ps1 이 dot-source 한다.
# 부르는 쪽이 먼저 $Assets(build 작업의 claude-board-windows 아티팩트 폴더)와 $Results(smoke-results 폴더)를 정해 둔다.
# 러너는 일회용 VM 이라 실제 %USERPROFILE%·%LOCALAPPDATA%·HKCU 를 그대로 쓴다(실제 사용자 환경과 가장 가깝다).
# 개발 기기에서 돌리면 실제 Claude Code 설정·Claude Board 설치를 지우므로 돌리지 않는다.
# PowerShell 7(pwsh) 에서 돈다. Windows PowerShell 5.1 에서 도는 것은 ps51-install.ps1 하나뿐이다.

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
# 하위 프로그램(hook·powershell.exe)의 UTF-8 출력을 그대로 읽고, 로그도 UTF-8 로 쓴다
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
$OutputEncoding = [Text.UTF8Encoding]::new($false)

if (-not $IsWindows) { throw '이 확인 스크립트는 Windows 러너 전용입니다(실제 사용자 폴더·레지스트리를 지웁니다).' }

$Assets = (Resolve-Path -LiteralPath $Assets).Path
New-Item -ItemType Directory -Force -Path $Results | Out-Null
$Results = (Resolve-Path -LiteralPath $Results).Path

# windows/VERSION(예: 2.12) — Tauri(NSIS)는 semver(2.12.0)를 쓴다. build 작업의 "버전 읽기" 와 같은 규칙
$CbVersion = [IO.File]::ReadAllText((Join-Path $Assets 'VERSION')).Trim()
$CbSemver = switch -Regex ($CbVersion) {
  '^\d+\.\d+\.\d+$' { $CbVersion; break }
  '^\d+\.\d+$' { "$CbVersion.0"; break }
  default { "$CbVersion.0.0" }
}
$CbSetupExe = Join-Path $Assets "ClaudeBoard-$CbVersion-setup.exe"
$CbTemp = Join-Path $(if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { [IO.Path]::GetTempPath() }) 'cb-smoke'
New-Item -ItemType Directory -Force -Path $CbTemp | Out-Null

# --- 파일 약속(windows/docs/SPEC.md 1.2·2)과 설치 위치 ---
$CbClaudeDir = Join-Path $env:USERPROFILE '.claude'
$CbSettings = Join-Path $CbClaudeDir 'settings.json'
$CbCredentials = Join-Path $CbClaudeDir '.credentials.json'
$CbNotifier = Join-Path $CbClaudeDir 'notifier'
$CbSessions = Join-Path $CbNotifier 'sessions'
$CbHookCopy = Join-Path $CbNotifier 'claude-board-hook.exe'
$CbAppPath = Join-Path $CbNotifier 'app-path'
$CbPluginCopy = Join-Path $CbNotifier 'plugins\md-render'
$CbLocalBin = Join-Path $env:USERPROFILE '.local\bin'
$CbCmdFiles = @('claude-board.cmd', 'claude-board-tui.cmd')
# md-render 플러그인 - zip·setup.exe 모두 설치 폴더 아래 plugins\md-render\ 에 같은 상대 경로로 다섯 파일이 놓인다(SPEC 8.1).
# 보드가 시작할 때 이 폴더를 notifier\plugins\md-render 로 옮겨 쓰고 제거가 그 사본을 지운다
$CbPluginRel = 'plugins\md-render'
$CbPluginFiles = @('LICENSE', 'README.md', '.claude-plugin\plugin.json', 'hooks\hooks.json', 'hooks\register.tsx')
# setup.exe — tauri.windows.conf.json 의 installMode currentUser → Tauri NSIS 템플릿이 $LOCALAPPDATA\<productName> 에 깐다
$CbSetupDir = Join-Path $env:LOCALAPPDATA 'Claude Board'
# install.ps1 — %LOCALAPPDATA%\Programs\Claude Board (setup.exe 로 깐 폴더가 있으면 그쪽)
$CbScriptDir = Join-Path $env:LOCALAPPDATA 'Programs\Claude Board'
$CbRunKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$CbRunValue = 'ClaudeBoard'   # crates/hook/src/setup.rs 의 RUN_VALUE. Tauri 제거 프로그램이 지우는 값은 productName("Claude Board")이라 다르다
$CbUninstKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Claude Board'
$CbManuKey = 'HKCU:\Software\claude\Claude Board'   # Tauri NSIS 의 MANUPRODUCTKEY — publisher 기본값은 identifier(com.claude.board)의 두 번째 조각
$CbHookCommand = $CbHookCopy.Replace('\', '/')     # SPEC 3.1 — exec form 명령은 슬래시로 쓴다
$CbHookMarkers = @('.claude/notifier/', '.claude\notifier\')

# SPEC 3.1 의 이벤트 표(crates/core/src/hooks.rs 의 HOOKS) — 이벤트, matcher(없으면 ''), hook 마다의 인자('|' 로 구분)
$CbHookTable = @(
  @('SessionStart', '', 'state idle'),
  @('UserPromptSubmit', '', 'state working'),
  @('PreToolUse', '*', 'state working'),
  @('PostToolUse', '*', 'state working'),
  @('PermissionRequest', '*', 'state waiting'),
  @('Notification', '', 'notify notification|state waiting'),
  @('Stop', '', 'notify stop|state idle'),
  @('StopFailure', '', 'state idle'),
  @('SessionEnd', '', 'state ended'),
  @('SubagentStart', '', 'state subagent'),
  @('SubagentStop', '', 'state subagent')
)
$CbHookEntryCount = 13   # 이벤트 11개, Notification·Stop 은 hook 이 두 개

# 설치 전에 미리 둔 사용자 설정 — 다른 hook(같은 Stop 이벤트·다른 이벤트)·임의 키·env 가 설치·제거 뒤에도 남아야 한다
$CbBaselineSettings = @'
{
  "model": "ci-keep-model",
  "ciKeep": {
    "nested": [1, "둘"]
  },
  "env": {
    "CI_KEEP_ENV": "keep"
  },
  "hooks": {
    "Stop": [
      { "hooks": [ { "type": "command", "command": "echo ci-keep-stop" } ] }
    ],
    "PreCompact": [
      { "matcher": "manual", "hooks": [ { "type": "command", "command": "echo ci-keep-precompact" } ] }
    ]
  }
}
'@
# 지금 Claude Code 로그인 흉내 — 제거는 건드리면 안 된다(SPEC 3.7). 토큰 모양은 넣지 않는다(보드가 사용량 API 를 부르지 않게)
$CbBaselineCredentials = '{"ciKeep": "제거가 건드리면 안 되는 파일"}'

# 확인 결과 — 시나리오 사이에 나누는 값은 변수 대신 이 표들에 넣는다(scriptblock 안에서 변수에 대입하면 지역 변수가 된다)
$CbChecks = [System.Collections.Generic.List[object]]::new()
$CbCurrent = @{ Scenario = ''; Annotate = 'error' }
$CbFailedScenarios = [System.Collections.Generic.List[string]]::new()
$CbState = @{}

# ---------------------------------------------------------------- 기록·표

function Format-Cell {
  param([AllowNull()][object]$Value)
  if ($null -eq $Value) { return '(없음)' }
  if ($Value -is [bool]) { return $Value.ToString().ToLowerInvariant() }
  if ($Value -is [array]) { return '[' + ((@($Value) | ForEach-Object { Format-Cell $_ }) -join ', ') + ']' }
  $s = "$Value" -replace "`r?`n", ' ⏎ '
  if ($s.Length -gt 400) { $s = $s.Substring(0, 400) + '…' }
  return $s
}

# 확인 하나를 표에 남긴다. -Info 는 정보용(기대와 달라도 실패로 세지 않는다 — Windows 미확인 규칙 등)
function Add-Check {
  param(
    [Parameter(Mandatory = $true)][string]$Item,
    [AllowNull()][object]$Expected,
    [AllowNull()][object]$Actual,
    [bool]$Pass,
    [switch]$Info
  )
  $result = if ($Info) { if ($Pass) { '참고' } else { '참고(다름)' } } elseif ($Pass) { '통과' } else { '실패' }
  $row = [pscustomobject]@{
    Scenario = $CbCurrent.Scenario
    Item = $Item
    Expected = (Format-Cell $Expected)
    Actual = (Format-Cell $Actual)
    Result = $result
  }
  $CbChecks.Add($row)
  $mark = switch ($result) { '통과' { 'O' } '실패' { 'X' } default { 'i' } }
  Write-Host ("  [{0}] {1}`n        기대: {2}`n        실제: {3}" -f $mark, $Item, $row.Expected, $row.Actual)
  if ($result -eq '실패') {
    Write-Host ("::{0}::[{1}] {2} — 기대 {3} / 실제 {4}" -f $CbCurrent.Annotate, $CbCurrent.Scenario, $Item, $row.Expected, $row.Actual)
  }
}

# 기대값과 실제값을 글자로 바꿔 비교한다(대소문자 무시 — 경로 비교에 맞춘다)
function Test-Value {
  param([string]$Item, [AllowNull()][object]$Expected, [AllowNull()][object]$Actual, [switch]$Info)
  Add-Check -Item $Item -Expected $Expected -Actual $Actual -Pass ((Format-Cell $Expected) -eq (Format-Cell $Actual)) -Info:$Info
}

# 시나리오 하나 — 본문의 확인을 모두 남기고, 실패가 있거나 예외가 나면 상태를 덤프한다. 정리는 늘 한다.
# 실패해도 다음 시나리오로 간다(첫 CI 실행에서 한 번에 많은 정보를 얻으려고). 끝에서 Complete-Checks 가 실패를 모아 throw 한다
function Invoke-Scenario {
  param([string]$Name, [scriptblock]$Body, [scriptblock]$Cleanup)
  $CbCurrent.Scenario = $Name
  $start = $CbChecks.Count
  Write-Host ''
  Write-Host "::group::$Name"
  Write-Host "===== $Name ====="
  try {
    & $Body
  } catch {
    Add-Check -Item '예외 없이 끝남' -Expected '예외 없음' -Actual ("{0} @ {1}" -f $_.Exception.Message, $_.InvocationInfo.PositionMessage) -Pass $false
  }
  Write-Host '::endgroup::'
  $failed = @($CbChecks | Select-Object -Skip $start | Where-Object { $_.Result -eq '실패' })
  if ($failed.Count -gt 0) {
    $CbFailedScenarios.Add($Name)
    Write-Host "!!!!! $Name — 실패 $($failed.Count)건: $(($failed | ForEach-Object { $_.Item }) -join ' / ')"
    Show-State -Why $Name
  }
  if ($Cleanup) {
    try { & $Cleanup } catch { Write-Host "  (정리 중 오류: $($_.Exception.Message) @ $($_.InvocationInfo.PositionMessage))" }
  }
}

function Format-Md {
  param([AllowNull()][string]$Text)
  if ($null -eq $Text) { return '' }
  return ($Text -replace '\|', '\|' -replace "`r?`n", ' ')
}

# 결과 표를 로그·결과 폴더·작업 요약(GITHUB_STEP_SUMMARY)에 남긴다
function Write-CheckSummary {
  param([string]$Title, [string]$FileName)
  Write-Host ''
  Write-Host "===== $Title — 결과 표 (긴 값은 줄임 — 전체는 $FileName·작업 요약) ====="
  $cut = { param($s, $n) if ($s.Length -gt $n) { $s.Substring(0, $n - 1) + '…' } else { $s } }
  $CbChecks | Format-Table -AutoSize -Property @(
    @{ n = '통과'; e = { $_.Result } },
    @{ n = '시나리오'; e = { & $cut $_.Scenario 24 } },
    @{ n = '항목'; e = { & $cut $_.Item 60 } },
    @{ n = '기대'; e = { & $cut $_.Expected 60 } },
    @{ n = '실제'; e = { & $cut $_.Actual 60 } }
  ) | Out-String -Width 320 | Write-Host
  $md = [Text.StringBuilder]::new()
  [void]$md.AppendLine("### $Title")
  [void]$md.AppendLine('')
  [void]$md.AppendLine('| 시나리오 | 항목 | 기대 | 실제 | 통과 |')
  [void]$md.AppendLine('|---|---|---|---|---|')
  foreach ($c in $CbChecks) {
    [void]$md.AppendLine(('| {0} | {1} | {2} | {3} | {4} |' -f (Format-Md $c.Scenario), (Format-Md $c.Item), (Format-Md $c.Expected), (Format-Md $c.Actual), (Format-Md $c.Result)))
  }
  [IO.File]::WriteAllText((Join-Path $Results $FileName), $md.ToString(), [Text.UTF8Encoding]::new($false))
  if ($env:GITHUB_STEP_SUMMARY) {
    [IO.File]::AppendAllText($env:GITHUB_STEP_SUMMARY, $md.ToString() + "`n", [Text.UTF8Encoding]::new($false))
  }
}

# 실패한 시나리오가 있으면 throw — 작업(단계)을 실패로 만든다
function Complete-Checks {
  param([string]$Title)
  $failed = @($CbChecks | Where-Object { $_.Result -eq '실패' })
  if ($failed.Count -gt 0) {
    throw "$Title 실패 $($failed.Count)건 (시나리오: $($CbFailedScenarios -join ', ')). 위 표와 상태 덤프를 보세요."
  }
  Write-Host "$Title — 모두 통과"
}

# ---------------------------------------------------------------- 파일·프로세스 도구

function Write-Utf8 {
  param([string]$Path, [string]$Text)
  $dir = Split-Path -Parent $Path
  if ($dir) { New-Item -ItemType Directory -Force -Path $dir | Out-Null }
  [IO.File]::WriteAllText($Path, $Text, [Text.UTF8Encoding]::new($false))
}

# 다른 프로세스가 쓰는 중인 파일(board.log 등)도 읽는다
function Read-TextShared {
  param([string]$Path)
  $fs = [IO.File]::Open($Path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete)
  try {
    $sr = [IO.StreamReader]::new($fs, [Text.Encoding]::UTF8, $true)
    return $sr.ReadToEnd()
  } finally {
    $fs.Dispose()
  }
}

function Get-FileHashText {
  param([string]$Path)
  if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return $null }
  return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash
}

# 지우기 — 잠깐 잠긴 파일(막 끝난 프로세스의 exe 등)이 있으면 몇 번 다시 한다
function Remove-PathRetry {
  param([string]$Path)
  for ($i = 0; $i -lt 6; $i++) {
    if (-not (Test-Path -LiteralPath $Path)) { return }
    Remove-Item -LiteralPath $Path -Recurse -Force -ErrorAction SilentlyContinue
    if (-not (Test-Path -LiteralPath $Path)) { return }
    Start-Sleep -Milliseconds 500
  }
  Write-Host "  (지우지 못함: $Path)"
}

# 조건이 참(값)이 될 때까지 기다린다 — 시간이 다 되면 $null
function Wait-Until {
  param([scriptblock]$Condition, [double]$TimeoutSec = 30, [int]$IntervalMs = 200)
  $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSec)
  while ($true) {
    $value = & $Condition
    if ($value) { return $value }
    if ([DateTime]::UtcNow -ge $deadline) { return $null }
    Start-Sleep -Milliseconds $IntervalMs
  }
}

# 콘솔 프로그램을 실행해 끝날 때까지(시간 제한) 기다리고 표준 출력·오류를 UTF-8 로 받는다. 표준 입력은 리디렉션해 바로 닫는다
function Invoke-Native {
  param([string]$File, [string[]]$Arguments = @(), [int]$TimeoutSec = 120, [string]$StdIn = '', [string[]]$UnsetEnv = @())
  $psi = [Diagnostics.ProcessStartInfo]::new($File)
  foreach ($a in $Arguments) { $psi.ArgumentList.Add($a) }
  foreach ($n in $UnsetEnv) { [void]$psi.Environment.Remove($n) }
  $psi.UseShellExecute = $false
  $psi.RedirectStandardInput = $true
  $psi.RedirectStandardOutput = $true
  $psi.RedirectStandardError = $true
  $psi.StandardOutputEncoding = [Text.UTF8Encoding]::new($false)
  $psi.StandardErrorEncoding = [Text.UTF8Encoding]::new($false)
  $shown = @($Arguments | ForEach-Object { if ($_ -match '\s') { [char]34 + $_ + [char]34 } else { $_ } }) -join ' '
  Write-Host "  실행: $File $shown"
  $p = [Diagnostics.Process]::Start($psi)
  if ($StdIn) { $p.StandardInput.Write($StdIn) }
  $p.StandardInput.Close()
  $outTask = $p.StandardOutput.ReadToEndAsync()
  $errTask = $p.StandardError.ReadToEndAsync()
  $exited = $p.WaitForExit($TimeoutSec * 1000)
  if (-not $exited) {
    Write-Host "  ($TimeoutSec 초가 지나 프로세스 트리를 끝낸다)"
    try { $p.Kill($true) } catch { Write-Host "  (끝내지 못함: $($_.Exception.Message))" }
  }
  # 손자 프로세스가 출력 파이프를 물려받아 쥐고 있으면 읽기가 끝나지 않는다 — 그것도 알린다
  $drained = [Threading.Tasks.Task]::WaitAll([Threading.Tasks.Task[]]@($outTask, $errTask), 15000)
  return [pscustomobject]@{
    ExitCode = $(if ($exited) { $p.ExitCode } else { $null })
    TimedOut = (-not $exited)
    PipesClosed = $drained
    Out = $(if ($outTask.IsCompleted) { $outTask.Result } else { '(출력 파이프가 닫히지 않음)' })
    Err = $(if ($errTask.IsCompleted) { $errTask.Result } else { '(오류 파이프가 닫히지 않음)' })
  }
}

# GUI 프로그램(설치·제거 프로그램)을 띄워 끝날 때까지 기다리고 종료 코드를 돌려준다
function Start-AndWait {
  param([string]$File, [string]$ArgumentString, [int]$TimeoutSec = 300)
  Write-Host "  실행: `"$File`" $ArgumentString"
  $p = Start-Process -FilePath $File -ArgumentList $ArgumentString -PassThru
  $null = $p.Handle   # 끝난 뒤 ExitCode 를 읽으려면 핸들을 먼저 잡아 둔다
  if (-not $p.WaitForExit($TimeoutSec * 1000)) {
    Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
    throw "$File 이 $TimeoutSec 초 안에 끝나지 않았습니다."
  }
  $p.WaitForExit()
  Write-Host "  종료 코드: $($p.ExitCode)"
  return $p.ExitCode
}

# ClaudeBoard.exe(보드·알림 패널) 목록 — 명령줄로 보드와 패널을 가른다
function Get-BoardProcesses {
  return @(Get-CimInstance Win32_Process -Filter "Name='ClaudeBoard.exe'" -ErrorAction SilentlyContinue |
      ForEach-Object { [pscustomobject]@{ Pid = [int]$_.ProcessId; Path = $_.ExecutablePath; CommandLine = $_.CommandLine } })
}

function Test-ProcessAlive {
  param([int]$ProcessId)
  return ($null -ne (Get-Process -Id $ProcessId -ErrorAction SilentlyContinue))
}

function Stop-ClaudeBoardProcesses {
  $ps = @(Get-Process -Name ClaudeBoard -ErrorAction SilentlyContinue)
  if ($ps.Count -eq 0) { return }
  Write-Host "  -- 떠 있는 ClaudeBoard.exe $($ps.Count)개를 끝낸다: $(($ps | ForEach-Object { $_.Id }) -join ', ')"
  $ps | Stop-Process -Force -ErrorAction SilentlyContinue
  $ps | Wait-Process -Timeout 15 -ErrorAction SilentlyContinue
}

# 이 확인이 띄운 가짜 claude.exe·NSIS 제거 프로그램 사본(%TEMP% 의 Un_A.exe·Au_.exe)
function Stop-HelperProcesses {
  $fakes = @(Get-Process -Name claude -ErrorAction SilentlyContinue | Where-Object { $_.Path -and $_.Path.StartsWith($CbTemp, [StringComparison]::OrdinalIgnoreCase) })
  $uninst = @(Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.ProcessName -match '^(Un_[A-Z]|Au_)$' })
  foreach ($p in $fakes + $uninst) {
    Write-Host "  -- $($p.ProcessName)($($p.Id)) 끝내기"
    Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
  }
}

# ---------------------------------------------------------------- 상태 덤프(실패했을 때)

function Show-File {
  param([string]$Path, [int]$Tail = 0)
  if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { Write-Host "  [파일] $Path — 없음"; return }
  Write-Host "  [파일] $Path"
  try {
    $lines = @((Read-TextShared $Path) -split "`r?`n")
    if ($Tail -gt 0 -and $lines.Count -gt $Tail) { $lines = $lines[($lines.Count - $Tail)..($lines.Count - 1)] }
    foreach ($l in $lines) { Write-Host "    | $l" }
  } catch {
    Write-Host "    (읽지 못함: $($_.Exception.Message))"
  }
}

function Show-Dir {
  param([string]$Path, [switch]$Recurse)
  if (-not (Test-Path -LiteralPath $Path)) { Write-Host "  [폴더] $Path — 없음"; return }
  Write-Host "  [폴더] $Path"
  Get-ChildItem -LiteralPath $Path -Force -Recurse:$Recurse -ErrorAction SilentlyContinue | ForEach-Object {
    $size = if ($_.PSIsContainer) { '<DIR>' } else { $_.Length }
    Write-Host ("    {0,-70} {1,12} {2:yyyy-MM-dd HH:mm:ss}" -f $_.FullName.Substring($Path.Length), $size, $_.LastWriteTime)
  }
}

function Show-Registry {
  param([string]$Key)
  if (-not (Test-Path -LiteralPath $Key)) { Write-Host "  [레지스트리] $Key — 없음"; return }
  $item = Get-Item -LiteralPath $Key
  Write-Host "  [레지스트리] $Key"
  foreach ($n in $item.GetValueNames()) {
    $name = if ($n) { $n } else { '(기본값)' }
    Write-Host ("    {0} = {1}" -f $name, $item.GetValue($n))
  }
}

function Show-State {
  param([string]$Why = '')
  Write-Host ''
  Write-Host "----- 상태 덤프: $Why -----"
  Show-File $CbSettings
  Get-ChildItem -LiteralPath $CbClaudeDir -Filter 'settings.json.bak-claude-board-*' -File -ErrorAction SilentlyContinue |
    ForEach-Object { Write-Host "  [백업] $($_.Name) ($($_.Length) B)" }
  Show-File $CbCredentials
  foreach ($d in @($CbSetupDir, $CbScriptDir)) { Show-Dir $d }
  foreach ($k in @($CbRunKey, $CbUninstKey, $CbManuKey)) { Show-Registry $k }
  Show-Dir $CbNotifier -Recurse
  Show-File $CbAppPath
  foreach ($f in @(Get-ChildItem -LiteralPath $CbSessions -Filter '*.json' -File -ErrorAction SilentlyContinue)) { Show-File $f.FullName }
  Show-File (Join-Path $CbNotifier 'board.log') -Tail 80
  Show-File (Join-Path $CbNotifier 'update.log') -Tail 80
  Show-Dir $CbLocalBin
  foreach ($n in $CbCmdFiles) { Show-File (Join-Path $CbLocalBin $n) }
  Write-Host '  [프로세스]'
  Get-CimInstance Win32_Process -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -match '^(ClaudeBoard|claude|claude-board-hook|uninstall|Un_[A-Z]|Au_|ClaudeBoard-.*-setup)\.exe$' } |
    ForEach-Object { Write-Host ("    {0,6} (부모 {1,6}) {2}" -f $_.ProcessId, $_.ParentProcessId, $_.CommandLine) }
  Write-Host '----- 상태 덤프 끝 -----'
}

# ---------------------------------------------------------------- 설치 상태 만들기

# 처음 설치 전 상태 — 설치·hook·자동 실행·notifier·명령을 모두 지우고, 미리 둔 사용자 설정만 남긴다.
# 제품의 제거 경로를 시험하는 곳이 아니므로 남은 것을 직접 지운다
function Reset-Machine {
  Write-Host '  -- 처음 설치 전 상태로 되돌린다'
  Stop-ClaudeBoardProcesses
  Stop-HelperProcesses
  foreach ($d in @($CbSetupDir, $CbScriptDir)) { Remove-PathRetry $d }
  foreach ($k in @($CbUninstKey, $CbManuKey)) { Remove-Item -LiteralPath $k -Recurse -Force -ErrorAction SilentlyContinue }
  foreach ($v in @($CbRunValue, 'Claude Board')) { Remove-ItemProperty -LiteralPath $CbRunKey -Name $v -ErrorAction SilentlyContinue }
  Remove-PathRetry $CbNotifier
  Remove-PathRetry (Join-Path $CbClaudeDir 'projects')
  Get-ChildItem -LiteralPath $CbClaudeDir -Filter 'settings.json.bak-claude-board-*' -File -ErrorAction SilentlyContinue | Remove-Item -Force
  foreach ($n in $CbCmdFiles) { Remove-PathRetry (Join-Path $CbLocalBin $n) }
  # 보드 설정(Tauri app config dir)·WebView2 자료·알림 패널 슬롯 — 화면 확인이 매번 같은 처음 상태에서 시작하게
  foreach ($d in @((Join-Path $env:APPDATA 'com.claude.board'), (Join-Path $env:LOCALAPPDATA 'com.claude.board'), (Join-Path ([IO.Path]::GetTempPath()) 'claude-notifier-slots'))) {
    Remove-PathRetry $d
  }
  Write-Utf8 $CbSettings $CbBaselineSettings
  Write-Utf8 $CbCredentials $CbBaselineCredentials
}

function Install-WithSetup {
  param([string]$ArgumentString = '/S')
  if (-not (Test-Path -LiteralPath $CbSetupExe -PathType Leaf)) { throw "설치 프로그램이 없습니다: $CbSetupExe" }
  return (Start-AndWait -File $CbSetupExe -ArgumentString $ArgumentString -TimeoutSec 300)
}

# NSIS 가 기록한 설치 폴더(Uninstall 키의 InstallLocation, 따옴표 포함)
function Get-InstallLocation {
  $v = Get-RegValue $CbUninstKey 'InstallLocation'
  if ($null -eq $v) { return $null }
  return "$v".Trim('"')
}

function Get-RegValue {
  param([string]$Key, [string]$Name)
  $k = Get-Item -LiteralPath $Key -ErrorAction SilentlyContinue
  if ($null -eq $k) { return $null }
  return $k.GetValue($Name)
}

function Get-RunValue {
  return (Get-RegValue $CbRunKey $CbRunValue)
}

function Test-SetupInstalled {
  return ((Test-Path -LiteralPath (Join-Path $CbSetupDir 'ClaudeBoard.exe')) -and
    (Test-Path -LiteralPath (Join-Path $CbSetupDir 'uninstall.exe')) -and
    (Test-Path -LiteralPath $CbUninstKey) -and
    (Test-Path -LiteralPath $CbHookCopy) -and
    ((Get-OurHookEntryCount) -eq $CbHookEntryCount))
}

# 앞 시나리오가 실패해 설치 상태가 깨졌으면 처음부터 다시 setup.exe /S 로 깐다
function Initialize-SetupInstall {
  if (Test-SetupInstalled) { return }
  Write-Host '  -- setup.exe 로 설치된 상태가 아니어서(앞 시나리오 실패?) 처음부터 다시 깐다'
  Reset-Machine
  $code = Install-WithSetup
  if ($code -ne 0) { throw "setup.exe /S 가 실패했습니다(종료 코드 $code)." }
  if (-not (Test-SetupInstalled)) { throw 'setup.exe /S 뒤에도 설치 상태가 아닙니다.' }
}

# ---------------------------------------------------------------- settings.json

function Read-Settings {
  if (-not (Test-Path -LiteralPath $CbSettings -PathType Leaf)) { return $null }
  return ((Read-TextShared $CbSettings) | ConvertFrom-Json -Depth 64)
}

function Test-OurHook {
  param([AllowNull()][object]$Hook)
  $c = "$($Hook.command)"
  foreach ($m in $CbHookMarkers) { if ($c.Contains($m)) { return $true } }
  return $false
}

# 그 이벤트에서 우리 hook 이 든 묶음
function Get-OurGroups {
  param([AllowNull()][object]$Settings, [string]$EventName)
  if ($null -eq $Settings -or $null -eq $Settings.hooks) { return @() }
  $prop = $Settings.hooks.PSObject.Properties[$EventName]
  if ($null -eq $prop) { return @() }
  return @(@($prop.Value) | Where-Object { @($_.hooks | Where-Object { Test-OurHook $_ }).Count -gt 0 })
}

function Get-OurHookEntryCount {
  $s = Read-Settings
  if ($null -eq $s -or $null -eq $s.hooks) { return 0 }
  $n = 0
  foreach ($p in $s.hooks.PSObject.Properties) {
    foreach ($g in @($p.Value)) { $n += @($g.hooks | Where-Object { Test-OurHook $_ }).Count }
  }
  return $n
}

function Format-HookGroup {
  param([object]$Group)
  $m = if ($Group.PSObject.Properties['matcher']) { $Group.matcher } else { '없음' }
  $hs = foreach ($h in @($Group.hooks)) {
    $form = if ($h.PSObject.Properties['args']) { (@($h.args) -join ' ') } else { '(args 없음 — exec form 아님)' }
    "$($h.type):$($h.command) $form (timeout $($h.timeout))"
  }
  return "matcher=$m; " + ($hs -join ' | ')
}

# SPEC 3.1 의 등록 형식 — 이벤트마다 우리 묶음이 하나, matcher·인자·timeout·exec form 명령이 표와 같은지
function Test-HookRegistration {
  $s = Read-Settings
  foreach ($row in $CbHookTable) {
    $name, $matcher, $spec = $row
    $m = if ($matcher) { $matcher } else { '없음' }
    $expected = "matcher=$m; " + ((($spec -split '\|') | ForEach-Object { "command:$CbHookCommand $_ (timeout 5)" }) -join ' | ')
    $actual = (@(Get-OurGroups $s $name) | ForEach-Object { Format-HookGroup $_ }) -join ' || '
    Test-Value "hook 등록: $name" $expected $actual
  }
  Test-Value 'Claude Board hook 명령 수(이벤트 11개)' $CbHookEntryCount (Get-OurHookEntryCount)
  $envValue = if ($s -and $s.env) { $s.env.CLAUDE_CODE_DISABLE_TERMINAL_TITLE } else { $null }
  Test-Value 'env.CLAUDE_CODE_DISABLE_TERMINAL_TITLE' '1' $envValue
}

function Test-HooksRemoved {
  $s = Read-Settings
  Test-Value 'Claude Board hook 명령 수' 0 (Get-OurHookEntryCount)
  $envValue = if ($s -and $s.env) { $s.env.CLAUDE_CODE_DISABLE_TERMINAL_TITLE } else { $null }
  Test-Value 'env.CLAUDE_CODE_DISABLE_TERMINAL_TITLE 지워짐' $null $envValue
}

# 미리 둔 사용자 설정이 그대로인지
function Test-PreservedSettings {
  $s = Read-Settings
  if ($null -eq $s) { Add-Check -Item 'settings.json 이 있음' -Expected '있음' -Actual '없음' -Pass $false; return }
  Test-Value '원래 설정 보존: model' 'ci-keep-model' $s.model
  Test-Value '원래 설정 보존: ciKeep' '{"nested":[1,"둘"]}' (ConvertTo-Json -InputObject $s.ciKeep -Compress -Depth 10)
  Test-Value '원래 설정 보존: env.CI_KEEP_ENV' 'keep' $(if ($s.env) { $s.env.CI_KEEP_ENV } else { $null })
  $stop = if ($s.hooks -and $s.hooks.PSObject.Properties['Stop']) { @(@($s.hooks.Stop) | ForEach-Object { @($_.hooks) } | Where-Object { $_.command -eq 'echo ci-keep-stop' }) } else { @() }
  Test-Value '원래 설정 보존: 다른 프로그램의 Stop hook' 1 $stop.Count
  $pc = if ($s.hooks -and $s.hooks.PSObject.Properties['PreCompact']) { ConvertTo-Json -InputObject @($s.hooks.PreCompact) -Compress -Depth 10 } else { $null }
  Test-Value '원래 설정 보존: PreCompact hook 묶음' '[{"matcher":"manual","hooks":[{"type":"command","command":"echo ci-keep-precompact"}]}]' $pc
}

function Get-SettingsBackups {
  return @(Get-ChildItem -LiteralPath $CbClaudeDir -Filter 'settings.json.bak-claude-board-*' -File -ErrorAction SilentlyContinue | Sort-Object LastWriteTimeUtc)
}

# notifier 에서 제거가 지워야 하는 것(SPEC 3.7, setup.rs 의 uninstall, md-render 사본 plugins) 가운데 남은 것
function Get-NotifierLeftovers {
  $names = @('claude-board-hook.exe', 'app-path', 'notify-position', 'sessions', 'accounts.json', 'accounts', 'plugins')
  $left = @($names | Where-Object { Test-Path -LiteralPath (Join-Path $CbNotifier $_) })
  $left += @(Get-ChildItem -LiteralPath $CbNotifier -Filter 'claude-board-hook.old-*' -ErrorAction SilentlyContinue | ForEach-Object { $_.Name })
  return $left
}

# 설치 폴더 아래 plugins\md-render\ 의 다섯 파일이 있는지(Expect 가 false 면 없는지) 확인한다
function Test-PluginFiles {
  param([string]$Root, [string]$Label, [bool]$Expect = $true)
  foreach ($f in $CbPluginFiles) {
    Test-Value "$Label 에 $CbPluginRel\$f" $Expect (Test-Path -LiteralPath (Join-Path $Root "$CbPluginRel\$f") -PathType Leaf)
  }
}

# 보드가 옮겨 쓴 사본을 흉내 낸다 - 다섯 파일과 Claude Code 가 플러그인 폴더에 만드는 tsconfig.json·.claude-plugin\types
function New-PluginCopy {
  foreach ($f in $CbPluginFiles) { Write-Utf8 (Join-Path $CbPluginCopy $f) "CI 사본 $f" }
  Write-Utf8 (Join-Path $CbPluginCopy 'tsconfig.json') '{}'
  Write-Utf8 (Join-Path $CbPluginCopy '.claude-plugin\types\index.d.ts') '// CI'
}

# ---------------------------------------------------------------- 가짜 Claude Code

function New-Project {
  param([string]$Name)
  $d = Join-Path $CbTemp "projects\$Name"
  New-Item -ItemType Directory -Force -Path $d | Out-Null
  return $d
}

# 대화 기록 파일 — 보드가 모델·컨텍스트를 읽는다. 경로 규칙은 SPEC 5.1(cwd 의 영숫자 밖 문자를 - 로)
function New-Transcript {
  param([string]$Cwd, [string]$SessionId)
  $slug = $Cwd -replace '[^A-Za-z0-9]', '-'
  $path = Join-Path $CbClaudeDir "projects\$slug\$SessionId.jsonl"
  $line = '{"type":"assistant","message":{"model":"claude-opus-5-5","role":"assistant","content":[{"type":"text","text":"CI"}],"usage":{"input_tokens":1200,"cache_creation_input_tokens":0,"cache_read_input_tokens":30000,"output_tokens":40}},"effort":"high"}'
  Write-Utf8 $path ($line + "`n")
  return $path
}

# node.exe 를 claude.exe 로 복사해 fake-claude.cjs 를 새 콘솔 창에서 띄운다(실제 사용자처럼 터미널 창이 있는 세션)
function Start-FakeClaude {
  param([string]$WorkDir)
  $bin = Join-Path $CbTemp 'fake-bin'
  $exe = Join-Path $bin 'claude.exe'
  if (-not (Test-Path -LiteralPath $exe)) {
    $node = (Get-Command node.exe -ErrorAction Stop).Source
    New-Item -ItemType Directory -Force -Path $bin | Out-Null
    Copy-Item -LiteralPath $node -Destination $exe
    Write-Host "  가짜 claude.exe: $node → $exe"
  }
  $dir = Join-Path $CbTemp ('fake-' + [guid]::NewGuid().ToString('N').Substring(0, 8))
  New-Item -ItemType Directory -Force -Path $dir | Out-Null
  $js = Join-Path $PSScriptRoot 'fake-claude.cjs'
  $p = Start-Process -FilePath $exe -ArgumentList ('"{0}" "{1}"' -f $js, $dir) -WorkingDirectory $WorkDir -PassThru
  $readyFile = Join-Path $dir 'ready.json'
  $ok = Wait-Until -TimeoutSec 30 -Condition { (Test-Path -LiteralPath $readyFile) -or $p.HasExited }
  if (-not $ok -or -not (Test-Path -LiteralPath $readyFile)) {
    Show-File (Join-Path $dir 'fake-claude.log')
    throw "가짜 claude.exe 가 준비되지 않았습니다(pid $($p.Id), 끝남 $($p.HasExited))."
  }
  $ready = (Read-TextShared $readyFile) | ConvertFrom-Json
  Write-Host "  가짜 claude.exe 시작: pid $($p.Id) (스스로 알린 pid $($ready.pid)), 작업 폴더 $WorkDir"
  $fake = [pscustomobject]@{ Dir = $dir; Exe = $exe; Process = $p; Pid = $p.Id; Ready = $ready; Next = 0 }
  if (-not $CbState.ContainsKey('Fakes')) { $CbState.Fakes = [System.Collections.Generic.List[object]]::new() }
  $CbState.Fakes.Add($fake)
  return $fake
}

function Stop-FakeClaude {
  param([AllowNull()][object]$Fake)
  if ($null -eq $Fake) { return }
  New-Item -ItemType File -Force -Path (Join-Path $Fake.Dir 'stop') | Out-Null
  if (-not $Fake.Process.WaitForExit(5000)) { Stop-Process -Id $Fake.Pid -Force -ErrorAction SilentlyContinue }
}

function Stop-AllFakeClaude {
  if ($CbState.ContainsKey('Fakes')) { foreach ($f in $CbState.Fakes) { Stop-FakeClaude $f } }
}

# hook 입력(stdin JSON) — Claude Code 문서의 공통 필드 + 이벤트별 필드
function New-HookInput {
  param([string]$EventName, [string]$SessionId, [string]$Cwd, [hashtable]$Extra = @{})
  $o = [ordered]@{
    session_id = $SessionId
    transcript_path = (New-Transcript -Cwd $Cwd -SessionId $SessionId)
    cwd = $Cwd
    hook_event_name = $EventName
  }
  foreach ($k in $Extra.Keys) { $o[$k] = $Extra[$k] }
  return $o
}

# settings.json 에 등록된 그 이벤트의 Claude Board hook 명령을 등록된 그대로(command·args·timeout) 실행할 단계로 만든다
function Get-HookSteps {
  param([string]$EventName, [System.Collections.IDictionary]$HookInput, [string]$Cwd)
  $s = Read-Settings
  $json = ConvertTo-Json -InputObject $HookInput -Compress -Depth 10
  $steps = [System.Collections.Generic.List[object]]::new()
  foreach ($g in @(Get-OurGroups $s $EventName)) {
    foreach ($h in @($g.hooks | Where-Object { Test-OurHook $_ })) {
      $timeout = if ($h.PSObject.Properties['timeout'] -and $h.timeout) { [int]$h.timeout } else { 60 }
      $steps.Add([ordered]@{
        label = "$EventName / $(@($h.args) -join ' ')"
        command = "$($h.command)"
        args = @($h.args | ForEach-Object { "$_" })
        timeoutMs = $timeout * 1000
        cwd = $Cwd
        input = $json
      })
    }
  }
  if ($steps.Count -eq 0) { throw "settings.json 에 $EventName 의 Claude Board hook 이 없습니다." }
  return $steps.ToArray()
}

function Invoke-FakeClaude {
  param([object]$Fake, [object[]]$Steps, [switch]$ReadTitle)
  $n = $Fake.Next
  $Fake.Next = $n + 1
  $cmd = [ordered]@{ steps = @($Steps); readTitle = [bool]$ReadTitle }
  $cmdPath = Join-Path $Fake.Dir "cmd-$n.json"
  Write-Utf8 "$cmdPath.tmp" (ConvertTo-Json -InputObject $cmd -Depth 10)
  Move-Item -LiteralPath "$cmdPath.tmp" -Destination $cmdPath -Force
  $resPath = Join-Path $Fake.Dir "res-$n.json"
  $budget = 20 + (($Steps | ForEach-Object { $_.timeoutMs } | Measure-Object -Sum).Sum / 1000)
  $null = Wait-Until -TimeoutSec $budget -IntervalMs 100 -Condition { (Test-Path -LiteralPath $resPath) -or $Fake.Process.HasExited }
  if (-not (Test-Path -LiteralPath $resPath)) {
    Show-File (Join-Path $Fake.Dir 'fake-claude.log')
    throw "가짜 claude.exe 가 $budget 초 안에 답하지 않았습니다(끝남 $($Fake.Process.HasExited))."
  }
  return ((Read-TextShared $resPath) | ConvertFrom-Json)
}

# 이벤트 하나를 가짜 Claude 아래에서 등록된 hook 으로 돌리고 결과를 보여 준다
function Send-HookEvent {
  param([object]$Fake, [string]$EventName, [string]$SessionId, [string]$Cwd, [hashtable]$Extra = @{}, [switch]$ReadTitle)
  $in = New-HookInput -EventName $EventName -SessionId $SessionId -Cwd $Cwd -Extra $Extra
  $steps = Get-HookSteps -EventName $EventName -HookInput $in -Cwd $Cwd
  Write-Host "  → $EventName ($SessionId)"
  $res = Invoke-FakeClaude -Fake $Fake -Steps $steps -ReadTitle:$ReadTitle
  foreach ($r in @($res.results)) {
    Write-Host ("    {0}: 종료 {1}, {2}ms, 오류 {3}, stdout {4}자, stderr {5}자" -f $r.label, (Format-Cell $r.status), $r.ms, (Format-Cell $r.error), "$($r.stdout)".Length, "$($r.stderr)".Length)
    if ($r.stderr) { Write-Host "      stderr: $($r.stderr)" }
  }
  if ($res.PSObject.Properties['error'] -and $res.error) { Write-Host "    가짜 claude 오류: $($res.error)" }
  return $res
}

# hook 이 등록된 그대로 실행되어 0 으로, 시간 안에, 아무것도 출력하지 않고 끝났는지(SPEC 3.2 — hook 은 Claude Code 를 막지 않는다)
function Test-HookResults {
  param([object]$Res)
  foreach ($r in @($Res.results)) {
    $ok = ($r.status -eq 0) -and (-not $r.error) -and ($r.ms -lt $r.timeoutMs) -and (-not $r.stdout) -and (-not $r.stderr)
    $actual = "종료 $(Format-Cell $r.status) · $($r.ms)ms · 오류 $(Format-Cell $r.error) · stdout '$($r.stdout)' · stderr '$($r.stderr)'"
    Add-Check -Item "hook 실행: $($r.label)" -Expected "종료 0 · $($r.timeoutMs)ms 안 · 출력 없음" -Actual $actual -Pass $ok
  }
}

function Read-Session {
  param([string]$SessionId)
  $p = Join-Path $CbSessions "$SessionId.json"
  if (-not (Test-Path -LiteralPath $p -PathType Leaf)) { return $null }
  $text = Read-TextShared $p
  Write-Host "    세션 기록: $text"
  return ($text | ConvertFrom-Json)
}

# Python " ".join(s.split()) — state hook 의 collapse_ws
function Format-CollapsedWhitespace {
  param([string]$Text)
  return ((@($Text -split '\s+') | Where-Object { $_ }) -join ' ')
}

# ---------------------------------------------------------------- 창·화면

function Initialize-Win32 {
  if ('CbWin' -as [type]) { return }
  Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

public static class CbWin {
  public delegate bool EnumProc(IntPtr hWnd, IntPtr lParam);
  [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc f, IntPtr l);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll", EntryPoint = "GetWindowLongPtrW")] static extern IntPtr GetWindowLongPtr(IntPtr h, int i);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out RECT r);

  // 그 프로세스의 보이는 최상위 Tauri 창들 — Tao 의 숨은 16x16 도우미 창(Tao Thread Event Target·단일 실행 창)은 뺀다
  public static long[] VisibleWindows(uint pid) {
    var list = new List<long>();
    EnumWindows((h, l) => {
      uint p;
      GetWindowThreadProcessId(h, out p);
      if (p == pid && IsWindowVisible(h) && ClassName(h.ToInt64()) == "Tauri Window") list.Add(h.ToInt64());
      return true;
    }, IntPtr.Zero);
    return list.ToArray();
  }
  public static long Foreground() { return GetForegroundWindow().ToInt64(); }
  public static uint Pid(long h) { uint p; GetWindowThreadProcessId(new IntPtr(h), out p); return p; }
  public static long ExStyle(long h) { return GetWindowLongPtr(new IntPtr(h), -20).ToInt64(); }
  public static string Title(long h) { var sb = new StringBuilder(512); GetWindowTextW(new IntPtr(h), sb, sb.Capacity); return sb.ToString(); }
  public static string ClassName(long h) { var sb = new StringBuilder(256); GetClassNameW(new IntPtr(h), sb, sb.Capacity); return sb.ToString(); }
  public static string Rect(long h) {
    RECT r;
    if (!GetWindowRect(new IntPtr(h), out r)) return "";
    return string.Format("{0},{1} {2}x{3}", r.Left, r.Top, r.Right - r.Left, r.Bottom - r.Top);
  }
}
'@
}

function Format-Window {
  param([long]$Hwnd)
  if ($Hwnd -eq 0) { return '0(없음)' }
  return ("0x{0:X} pid={1} class={2} title='{3}' rect={4}" -f $Hwnd, [CbWin]::Pid($Hwnd), [CbWin]::ClassName($Hwnd), [CbWin]::Title($Hwnd), [CbWin]::Rect($Hwnd))
}

# 화면 전체 — DWM 이 합성한 화면을 찍으므로 겹친(layered) 창도 찍힌다
function Save-Screenshot {
  param([string]$Name)
  try {
    Add-Type -AssemblyName System.Windows.Forms, System.Drawing
    $b = [System.Windows.Forms.SystemInformation]::VirtualScreen
    $bmp = [System.Drawing.Bitmap]::new($b.Width, $b.Height)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    # SourceCopy|CaptureBlt 조합은 .NET 이 정의되지 않은 enum 값이라며 거부한다
    $g.CopyFromScreen($b.X, $b.Y, 0, 0, $b.Size)
    $path = Join-Path $Results $Name
    $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose()
    $bmp.Dispose()
    Write-Host "  스크린샷: $path ($($b.Width)x$($b.Height))"
    return $path
  } catch {
    Write-Host "  (스크린샷 실패: $($_.Exception.Message))"
    return $null
  }
}
