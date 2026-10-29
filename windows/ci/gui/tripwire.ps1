# 실제 사용자 환경을 건드리지 않았는지 지키는 철선 — GUI 시험 앞뒤로 돈다(windows/ci/gui/README.md).
# 지키는 것: (1) 사용자 settings.json 과 PowerShell profile.ps1 의 md5(해시만 기록한다. 내용은 읽어 출력하지 않는다),
#            (2) 시험 중에 진짜 claude 가 새로 뜨지 않는가 — 기준선 비교다. 족보(보드의 자손인가)를 따지지 않는다.
# PowerShell 5.1 과 7 모두에서 돈다.
#
#   pwsh -File tripwire.ps1 -Phase before   -State <상태.json> [-BoardPid <pid>] [-BoardFile <board.json>] [-FakeDir <가짜 claude 폴더>]
#   pwsh -File tripwire.ps1 -Phase after    -State <상태.json> [-BoardFile <board.json>]
#   pwsh -File tripwire.ps1 -Phase check    -State <상태.json>        (해시는 보지 않고 진짜 claude 만 본다 — 시험 도중에 언제든)
#   pwsh -File tripwire.ps1 -Phase selftest                            (임시 사본으로 변화 감지를 증명한다. 실제 파일은 읽지도 않는다)
#
# 인자
#   -State       상태 파일(JSON). before 가 쓰고 after·check 가 읽는다. 지키는 파일 안에 두지 않는다
#   -BoardPid    격리한 보드(ClaudeBoard.exe)의 pid. 알려 주는 용도(보드의 자손이면 REAL-CLAUDE 줄에 표시)이지 판정 조건이 아니다
#   -BoardFile   launch-board.ps1 이 쓴 board.json. 있으면 거기서 보드 pid 와 가짜 폴더를 읽는다(before 가 보드보다 먼저 돌아도 된다)
#   -FakeDir     가짜 claude(node 사본 claude.exe)가 든 폴더. 실행 파일이나 명령줄이 이 아래인 프로세스는 허용한다
#   -SettingsPath / -ProfilePaths   지킬 파일(기본: ~\.claude\settings.json, 문서\PowerShell\profile.ps1, 문서\WindowsPowerShell\profile.ps1)
#   -TestScopeUnder   철선 자체의 시험용 — 실행 파일이 이 폴더 아래인 프로세스만 claude 로 본다(시험이 같은 PC 의 실제 claude 세션에 흔들리지 않게).
#                     실제 GUI 시험에서는 쓰지 않는다. 쓰면 그 밖의 진짜 claude 를 보지 못한다
#
# claude 로 치는 것: 이름이 claude.exe 인 프로세스, 그리고 명령줄에 `@anthropic-ai/claude-code`(역슬래시도 같다)가 든 node.exe·bun.exe(npm 설치).
#   before 는 이미 도는 것의 pid + 만든 시각을 상태 파일에 기준선으로 적는다. after·check 는 기준선에 없는 새 것이 허용 폴더 밖이면 실패다.
#   보드가 이미 끝났어도 판정한다. 명령줄은 출력하지 않는다(경로와 종류만).
#
# 판정 — 종료코드가 답이다(출력을 파이프에 물리지 않는다)
#   0  바뀐 것 없음, 새로 뜬 진짜 claude 없음
#   1  지킨 파일의 md5 가 바뀌었거나, 기준선에 없는 새 claude(허용 폴더 밖)가 돈다
#   2  사용법 오류(상태 파일 없음, 상태 파일에 기준선이 없음 등)
# 출력 줄은 `TRIPWIRE|<종류>|…` 로 시작한다. OTHER-CLAUDE 는 시험 전부터 돌던 다른 세션의 claude 를 알리는 참고 줄이고 실패가 아니다.
param(
  [Parameter(Mandatory = $true)][ValidateSet('before', 'after', 'check', 'selftest')][string]$Phase,
  [string]$State,
  [int]$BoardPid = 0,
  [string]$BoardFile,
  [string]$FakeDir,
  [string]$TestScopeUnder,
  [string]$SettingsPath,
  [string[]]$ProfilePaths
)

$ErrorActionPreference = 'Stop'
# 한글 출력이 부르는 쪽(node·CI)에서 UTF-8 로 읽히게 한다
[Console]::OutputEncoding = New-Object Text.UTF8Encoding($false)

function Get-DefaultWatch {
  $docs = [Environment]::GetFolderPath('MyDocuments')
  $settings = $SettingsPath
  if (-not $settings) { $settings = Join-Path (Join-Path $env:USERPROFILE '.claude') 'settings.json' }
  $profiles = $ProfilePaths
  if (-not $profiles) {
    $profiles = @((Join-Path $docs 'PowerShell\profile.ps1'), (Join-Path $docs 'WindowsPowerShell\profile.ps1'))
  }
  $list = @([pscustomobject]@{ label = 'settings.json'; path = $settings })
  foreach ($p in $profiles) { $list += [pscustomobject]@{ label = 'profile:' + (Split-Path -Leaf (Split-Path -Parent $p)); path = $p } }
  return $list
}

# 파일 하나의 md5 — 없으면 ABSENT. 내용은 해시 계산 말고는 읽지 않는다
function Get-Md5([string]$Path) {
  if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return 'ABSENT' }
  # 모듈 없이 .NET 으로 계산한다(PSModulePath 가 어긋난 셸에서도 도는 것을 우선한다). 다른 프로그램이 쥐고 있어도 읽을 수 있게 공유 읽기로 연다
  $md5 = [Security.Cryptography.MD5]::Create()
  $stream = [IO.File]::Open($Path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::ReadWrite)
  try { $bytes = $md5.ComputeHash($stream) } finally { $stream.Dispose(); $md5.Dispose() }
  return ([BitConverter]::ToString($bytes) -replace '-', '').ToLowerInvariant()
}

function Get-ProcessTable {
  $table = @{}
  foreach ($p in (Get-CimInstance Win32_Process)) {
    $table[[int]$p.ProcessId] = [pscustomobject]@{ pid = [int]$p.ProcessId; ppid = [int]$p.ParentProcessId; name = $p.Name; path = $p.ExecutablePath; created = $p.CreationDate; commandLine = $p.CommandLine }
  }
  return $table
}

# pid 의 조상 사슬 — 부모가 자식보다 늦게 만들어졌으면 pid 재사용이라 끊는다
function Get-Ancestors($Table, [int]$ProcessId) {
  $chain = @()
  $seen = @{}
  $node = $Table[$ProcessId]
  while ($node -and -not $seen.ContainsKey($node.pid)) {
    $seen[$node.pid] = $true
    $parent = $Table[$node.ppid]
    if (-not $parent) { break }
    if ($parent.created -and $node.created -and $parent.created -gt $node.created) { break }
    $chain += $parent.pid
    $node = $parent
  }
  return $chain
}

function Test-UnderDir([string]$Path, [string]$Dir) {
  if (-not $Path -or -not $Dir) { return $false }
  $root = [IO.Path]::GetFullPath($Dir).TrimEnd('\') + '\'
  return [IO.Path]::GetFullPath($Path).StartsWith($root, [StringComparison]::OrdinalIgnoreCase)
}

# npm 설치 claude 의 명령줄 표지 — Windows 명령줄은 역슬래시라 둘 다 받는다
$ClaudeCodePackage = '@anthropic-ai[\\/]claude-code'

# claude 프로세스의 종류 — exe(이름이 claude.exe) · node(node/bun 이 claude-code 패키지를 돌린다) · 아니면 빈 문자열
function Get-ClaudeKind($Proc) {
  if ($Proc.name -ieq 'claude.exe') { return 'exe' }
  if (($Proc.name -ieq 'node.exe' -or $Proc.name -ieq 'bun.exe') -and $Proc.commandLine -and ($Proc.commandLine -match $ClaudeCodePackage)) { return 'node' }
  return ''
}

# 시각 문자열 — 같은 프로세스는 늘 같은 값이다(pid 재사용을 가르는 데 쓴다)
function Get-CreatedText($Proc) {
  if (-not $Proc.created) { return '' }
  return ([datetime]$Proc.created).ToUniversalTime().ToString('o')
}

# 허용(가짜) 인가 — 실행 파일이 허용 폴더 아래일 때만이다. 명령줄에 허용 폴더가 들었다는 이유로는 허용하지 않는다(진짜 claude 에 그런 인자를 붙여도 가려지지 않게)
function Test-Fake($Proc, [string]$Fake) {
  if (-not $Fake -or -not $Proc.path) { return $false }
  return (Test-UnderDir $Proc.path $Fake)
}

# 지금 도는 claude 전부 — pid·만든 시각·종류·경로
function Get-ClaudeProcesses($Table) {
  $list = @()
  foreach ($p in $Table.Values) {
    $kind = Get-ClaudeKind $p
    if (-not $kind) { continue }
    if ($TestScopeUnder -and -not (Test-UnderDir $p.path $TestScopeUnder)) { continue }
    $list += [pscustomobject]@{ pid = $p.pid; created = (Get-CreatedText $p); kind = $kind; path = $p.path; proc = $p }
  }
  return $list
}

# claude 를 가른다 — 기준선(before 때 이미 돌던 것)에 있으면 참고(other), 없고 허용 폴더 밖이면 진짜(real).
# $Board 가 살아 있고 그 자손이면 childOfBoard 로 표시만 한다(판정 조건이 아니다)
function Find-Claude($Baseline, [int]$Board, [string]$Fake) {
  $table = Get-ProcessTable
  $known = @{}
  # pwsh 7 의 ConvertFrom-Json 은 ISO 문자열을 DateTime 으로 바꿔 읽는다 — 같은 글자 꼴로 되돌려 비교한다
  foreach ($b in @($Baseline)) {
    if (-not $b) { continue }
    $when = $b.created
    if ($when -is [datetime]) { $when = $when.ToUniversalTime().ToString('o') }
    $known[('{0}|{1}' -f $b.pid, $when)] = $true
  }
  $real = @()
  $other = @()
  foreach ($c in (Get-ClaudeProcesses $table)) {
    if (Test-Fake $c.proc $Fake) { continue }
    if ($known.ContainsKey(('{0}|{1}' -f $c.pid, $c.created))) { $other += $c; continue }
    $child = ($Board -gt 0) -and ((Get-Ancestors $table $c.pid) -contains $Board)
    $real += [pscustomobject]@{ pid = $c.pid; created = $c.created; kind = $c.kind; path = $c.path; childOfBoard = $child }
  }
  return [pscustomobject]@{ real = $real; other = $other }
}

function Write-ClaudeReport($Found) {
  foreach ($p in $Found.other) { Write-Output ("TRIPWIRE|OTHER-CLAUDE|{0}|{1}|{2}" -f $p.pid, $p.path, $p.kind) }
  foreach ($p in $Found.real) { Write-Output ("TRIPWIRE|REAL-CLAUDE|{0}|{1}|{2}|childOfBoard={3}" -f $p.pid, $p.path, $p.kind, $p.childOfBoard) }
}

# board.json(launch-board.ps1 이 씀)에서 보드 pid 와 가짜 폴더를 읽는다 — 없거나 깨졌으면 빈 값
function Read-BoardFile([string]$Path) {
  $none = [pscustomobject]@{ pid = 0; fakeDir = '' }
  if (-not $Path -or -not (Test-Path -LiteralPath $Path -PathType Leaf)) { return $none }
  try {
    $rec = Get-Content -LiteralPath $Path -Raw -Encoding UTF8 | ConvertFrom-Json
    $boardPid = 0
    if ($rec.pid) { $boardPid = [int]$rec.pid }
    return [pscustomobject]@{ pid = $boardPid; fakeDir = [string]$rec.fakeDir }
  } catch {
    return $none
  }
}

function Read-State {
  if (-not $State -or -not (Test-Path -LiteralPath $State -PathType Leaf)) {
    Write-Output 'TRIPWIRE|USAGE|상태 파일이 없다 — 먼저 -Phase before -State <파일> 을 돌린다'
    exit 2
  }
  return (Get-Content -LiteralPath $State -Raw -Encoding UTF8 | ConvertFrom-Json)
}

function Invoke-Phase([string]$Which) {
  if ($Which -eq 'before') {
    if (-not $State) { Write-Output 'TRIPWIRE|USAGE|-State 가 필요하다'; exit 2 }
    $watch = @()
    foreach ($w in (Get-DefaultWatch)) {
      if (Test-UnderDir $State ([IO.Path]::GetDirectoryName($w.path))) { Write-Output ('TRIPWIRE|USAGE|상태 파일을 지키는 파일의 폴더 안에 두지 않는다: ' + $State); exit 2 }
      $watch += [pscustomobject]@{ label = $w.label; path = $w.path; md5 = (Get-Md5 $w.path) }
    }
    New-Item -ItemType Directory -Force -Path ([IO.Path]::GetDirectoryName([IO.Path]::GetFullPath($State))) | Out-Null
    $fromFile = Read-BoardFile $BoardFile
    $board = $BoardPid
    if ($board -le 0) { $board = $fromFile.pid }
    $fake = $FakeDir
    if (-not $fake) { $fake = $fromFile.fakeDir }
    # 기준선 — 지금 돌고 있는 claude 전부(가짜 포함). 이후 새로 생긴 것만 문제 삼는다
    $baseline = @()
    foreach ($c in (Get-ClaudeProcesses (Get-ProcessTable))) { $baseline += [pscustomobject]@{ pid = $c.pid; created = $c.created } }
    $doc = [pscustomobject]@{ takenAt = (Get-Date).ToString('o'); boardPid = $board; boardFile = $BoardFile; fakeDir = $fake; watch = $watch; claudeBaseline = $baseline }
    [IO.File]::WriteAllText([IO.Path]::GetFullPath($State), ($doc | ConvertTo-Json -Depth 5), (New-Object Text.UTF8Encoding($false)))
    foreach ($w in $watch) { Write-Output ('TRIPWIRE|RECORDED|{0}|{1}' -f $w.label, $w.md5) }
    Write-Output ('TRIPWIRE|BASELINE|{0}' -f $baseline.Count)
    # 보드를 이미 알면(보드가 먼저 떴다) 그 자손 가운데 허용 폴더 밖 claude 는 시작부터 어긋난 것이다
    $found = Find-Claude @() $board $fake
    $under = @($found.real | Where-Object { $_.childOfBoard })
    foreach ($c in $found.real) { if (-not $c.childOfBoard) { Write-Output ("TRIPWIRE|OTHER-CLAUDE|{0}|{1}|{2}" -f $c.pid, $c.path, $c.kind) } }
    foreach ($c in $under) { Write-Output ("TRIPWIRE|REAL-CLAUDE|{0}|{1}|{2}|childOfBoard=True" -f $c.pid, $c.path, $c.kind) }
    if ($under.Count -gt 0) { Write-Output 'TRIPWIRE|FAIL|보드 밑에 진짜 claude 가 이미 돈다'; exit 1 }
    Write-Output 'TRIPWIRE|OK|before'
    exit 0
  }

  $saved = Read-State
  if ($null -eq $saved.PSObject.Properties['claudeBaseline']) {
    Write-Output 'TRIPWIRE|USAGE|상태 파일에 claude 기준선이 없다 — -Phase before 를 다시 돈다'
    exit 2
  }
  $fromFile = Read-BoardFile $BoardFile
  if (-not $BoardFile -and $saved.boardFile) { $fromFile = Read-BoardFile ([string]$saved.boardFile) }
  $board = $BoardPid
  if ($board -le 0) { $board = $fromFile.pid }
  if ($board -le 0) { $board = [int]$saved.boardPid }
  $fake = $FakeDir
  if (-not $fake) { $fake = $fromFile.fakeDir }
  if (-not $fake) { $fake = [string]$saved.fakeDir }
  $failed = $false

  if ($Which -eq 'after') {
    foreach ($w in $saved.watch) {
      $now = Get-Md5 $w.path
      if ($now -ne $w.md5) {
        Write-Output ('TRIPWIRE|CHANGED|{0}|{1}|{2}' -f $w.label, $w.md5, $now)
        $failed = $true
      } else {
        Write-Output ('TRIPWIRE|SAME|{0}|{1}' -f $w.label, $now)
      }
    }
  }
  $found = Find-Claude $saved.claudeBaseline $board $fake
  Write-ClaudeReport $found
  if ($found.real.Count -gt 0) { $failed = $true }
  if ($failed) { Write-Output ('TRIPWIRE|FAIL|' + $Which); exit 1 }
  Write-Output ('TRIPWIRE|OK|' + $Which)
  exit 0
}

if ($Phase -ne 'selftest') { Invoke-Phase $Phase }

# ── selftest: 임시 폴더의 사본만 쓴다. 실제 settings.json·profile.ps1 은 읽지도 않는다 ─────────────────────
$tmp = Join-Path ([IO.Path]::GetTempPath()) ('tripwire-selftest-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $tmp | Out-Null
try {
  $settings = Join-Path $tmp 'home\.claude\settings.json'
  $profileFile = Join-Path $tmp 'docs\PowerShell\profile.ps1'
  New-Item -ItemType Directory -Force -Path (Split-Path -Parent $settings), (Split-Path -Parent $profileFile) | Out-Null
  [IO.File]::WriteAllText($settings, '{"selftest":1}')
  [IO.File]::WriteAllText($profileFile, '# selftest profile')
  $stateFile = Join-Path $tmp 'state\state.json'
  $self = $MyInvocation.MyCommand.Path
  $shell = (Get-Process -Id $PID).Path
  function Invoke-Self([string]$Which) {
    $out = & $shell -NoProfile -File $self -Phase $Which -State $stateFile -SettingsPath $settings -ProfilePaths $profileFile -TestScopeUnder $tmp 2>&1
    return [pscustomobject]@{ code = $LASTEXITCODE; text = ($out -join "`n") }
  }
  $b = Invoke-Self 'before'
  $clean = Invoke-Self 'after'
  [IO.File]::WriteAllText($profileFile, '# selftest profile, changed')
  $changed = Invoke-Self 'after'
  $ok = ($b.code -eq 0) -and ($clean.code -eq 0) -and ($changed.code -eq 1) -and ($changed.text -match 'TRIPWIRE\|CHANGED\|profile:')
  foreach ($r in @($b, $clean, $changed)) { Write-Output $r.text }
  if ($ok) { Write-Output 'TRIPWIRE|SELFTEST|PASS|임시 사본의 변화를 잡았고 바뀌지 않은 사본은 통과시켰다'; $code = 0 }
  else { Write-Output ('TRIPWIRE|SELFTEST|FAIL|before={0} clean={1} changed={2}' -f $b.code, $clean.code, $changed.code); $code = 1 }
} finally {
  Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
}
exit $code
