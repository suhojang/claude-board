param([string]$Version, [switch]$Update)
# Claude Board for Windows 설치·업데이트 — 처음 설치와 업데이트 모두 같은 명령이다
#   irm https://raw.githubusercontent.com/suhojang/claude-board/main/windows/packaging/install.ps1 | iex
# 릴리스 목록에서 windows-v 로 시작하는 최신 릴리스(또는 -Version 으로 고른 버전)의 zip 을 받아 SHA256 을 확인하고
# %LOCALAPPDATA%\Programs\Claude Board 에 바꿔 끼운 뒤, claude-board-hook.exe setup 으로 사용자 설정을 하고 보드를 다시 띄운다.
# 전역 settings.json 에 Claude Board hook 이 없을 때(처음 설치)만 hook 등록을 묻는다. -Update 면 묻지 않는다(보드의 업데이트 단추).
# /releases/latest 는 macOS 판 릴리스가 쓰므로 절대 쓰지 않는다(windows/docs/SPEC.md 0.1).
# Windows PowerShell 5.1 에서도 돌게 쓴다 — ??·삼항·&& 같은 PowerShell 7 문법을 쓰지 않는다.
# irm | iex 는 호출한 셸 안에서 돌므로 exit 를 쓰지 않는다(터미널이 닫힌다). 실패는 throw 로 알린다.
# 코드는 ASCII 로만 쓰고 한글은 문자열·주석에만 둔다. BOM 을 붙이지 않는다 — 문자열 앞의 BOM 은 param 블록을 깨뜨린다.

# 받는 도중 끊겨도 아무것도 실행되지 않게 전체를 함수로 감싸 마지막 줄에서 부른다
function Install-ClaudeBoard {
  param([string]$Version, [switch]$Update)
  $ErrorActionPreference = 'Stop'
  $ProgressPreference = 'SilentlyContinue'   # PowerShell 5.1 의 진행 막대는 내려받기를 크게 늦춘다

  $Repo = 'suhojang/claude-board'
  $ZipName = 'ClaudeBoard-windows-x64.zip'
  $UserAgent = 'ClaudeBoard-Windows-Installer'
  $Notifier = Join-Path $env:USERPROFILE '.claude\notifier'
  $Settings = Join-Path $env:USERPROFILE '.claude\settings.json'

  # 실제 빌드 번호 — 레지스트리를 먼저 본다. [Environment]::OSVersion 은 호환 매니페스트가 없는 프로세스에 6.2 같은 거짓 버전을 준다.
  # 못 읽으면 0 (검사하지 않고 넘어간다)
  function Get-OsBuild {
    $n = 0
    try {
      $s = (Get-ItemProperty -Path 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion' -Name CurrentBuildNumber -ErrorAction Stop).CurrentBuildNumber
      if (-not [int]::TryParse("$s", [ref]$n)) { $n = 0 }
    } catch { $n = 0 }
    if ($n -le 0) { try { $n = [Environment]::OSVersion.Version.Build } catch { $n = 0 } }
    return [int]$n
  }
  # 17763 = Windows 10 1809, ConPTY(CreatePseudoConsole)가 들어간 첫 빌드
  function Test-OsBuild([int]$Build) { return ($Build -ge 17763) }

  # 출력은 UTF-8 로 — 보드의 업데이트 단추는 이 출력을 notifier\update.log 로 받는다. 끝나면 원래대로 돌린다
  $prevEncoding = $null
  try { $prevEncoding = [Console]::OutputEncoding; [Console]::OutputEncoding = New-Object System.Text.UTF8Encoding $false } catch { }
  try {
    # PowerShell 5.1 은 기본으로 TLS 1.2 를 켜지 않을 수 있다
    try { [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12 } catch { }

    Write-Host '== Claude Board for Windows 설치 =='
    if (-not [Environment]::Is64BitOperatingSystem) { throw '64비트(x64) Windows 가 필요합니다.' }
    # 보드는 CreatePseudoConsole(ConPTY)을 정적으로 가져다 쓴다 — Windows 10 1809(빌드 17763) 미만은 실행 시 진입점 오류로 뜨지 않는다
    $build = Get-OsBuild
    if ($build -gt 0 -and -not (Test-OsBuild $build)) {
      throw "Windows 10 1809(빌드 17763) 이상이 필요합니다. 이 PC 는 빌드 $build 입니다. Windows 업데이트 후 다시 실행하세요."
    }

    # 처음 설치(hook 이 아직 없음)일 때만 묻는다 — hook 명령에 들어가는 표시(.claude/notifier/)로 판단한다
    $first = $true
    if (Test-Path -LiteralPath $Settings -PathType Leaf) {
      $text = [IO.File]::ReadAllText($Settings)
      if ($text.Contains('.claude/notifier/') -or $text.Contains('.claude\\notifier\\')) { $first = $false }
    }
    if ($first -and -not $Update) {
      Write-Host 'Claude Code 전역 설정 %USERPROFILE%\.claude\settings.json 에 Claude Board hook 11개와'
      Write-Host '환경변수 CLAUDE_CODE_DISABLE_TERMINAL_TITLE=1 을 등록합니다.'
      Write-Host '원본은 settings.json.bak-claude-board-<날짜-시각> 으로 백업하고, 다른 hook·설정은 그대로 둡니다.'
      Write-Host '이후 업데이트에서는 다시 묻지 않습니다.'
      $answer = $null
      if (-not [Console]::IsInputRedirected) {
        try { $answer = Read-Host '계속할까요? [Y/n]' } catch { $answer = $null }
      }
      if ($null -eq $answer) {
        throw '터미널(PowerShell 창)에서 설치 명령을 실행해야 hook 등록을 확인할 수 있습니다.'
      }
      if ($answer -ne '' -and $answer -notmatch '^[Yy]') {
        Write-Host '설치를 취소했습니다.'
        return
      }
    }

    # 1) 버전 정하기 — 릴리스 목록에서 windows-v 로 시작하는 첫 정식 릴리스
    $Version = ($Version -replace '^windows-v', '') -replace '^v', ''
    if (-not $Version) {
      Write-Host '1) 최신 버전 찾기'
      $releases = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases?per_page=30" -UserAgent $UserAgent -UseBasicParsing
      foreach ($r in $releases) {
        if (-not $r.draft -and -not $r.prerelease -and "$($r.tag_name)".StartsWith('windows-v')) {
          $Version = "$($r.tag_name)".Substring(9)
          break
        }
      }
      if (-not $Version) { throw 'Windows 판 릴리스(windows-v...)를 찾지 못했습니다.' }
    } else {
      Write-Host '1) 버전 지정'
    }
    Write-Host "   Claude Board for Windows $Version"
    $base = "https://github.com/$Repo/releases/download/windows-v$Version"

    $work = Join-Path ([IO.Path]::GetTempPath()) ('claude-board-' + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $work | Out-Null
    try {
      # 2) 받기 → SHA256 확인 → 풀기
      Write-Host '2) 받기·확인'
      $zip = Join-Path $work $ZipName
      $sha = Join-Path $work "$ZipName.sha256"
      Invoke-WebRequest -Uri "$base/$ZipName" -OutFile $zip -UserAgent $UserAgent -UseBasicParsing
      Invoke-WebRequest -Uri "$base/$ZipName.sha256" -OutFile $sha -UserAgent $UserAgent -UseBasicParsing
      $expected = ([IO.File]::ReadAllText($sha).Trim() -split '\s+')[0]
      $actual = (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash
      if ($expected -notmatch '^[0-9A-Fa-f]{64}$' -or $actual -ne $expected) {
        throw "받은 파일의 SHA256 이 릴리스에 적힌 값과 달라 설치하지 않습니다. (받은 파일 $actual / 릴리스 $expected)"
      }
      $stage = Join-Path $work 'new'
      Expand-Archive -LiteralPath $zip -DestinationPath $stage -Force
      foreach ($need in @('ClaudeBoard.exe', 'claude-board-hook.exe')) {
        if (-not (Test-Path -LiteralPath (Join-Path $stage $need) -PathType Leaf)) { throw "받은 zip 에 $need 가 없습니다." }
      }

      # 설치 위치 — setup.exe(NSIS)로 설치한 폴더가 있으면(app-path 가 가리키고 uninstall.exe 가 있음) 그 자리를 업데이트한다.
      # 둘로 나뉘면 설정 > 앱 목록과 실제 실행 파일이 어긋나므로. 그 밖에는 %LOCALAPPDATA%\Programs\Claude Board
      $dir = Join-Path $env:LOCALAPPDATA 'Programs\Claude Board'
      $appPathFile = Join-Path $Notifier 'app-path'
      if (Test-Path -LiteralPath $appPathFile -PathType Leaf) {
        $p = [IO.File]::ReadAllText($appPathFile).Trim()
        if ($p -and (Test-Path -LiteralPath $p -PathType Leaf)) {
          $pdir = Split-Path -Parent $p
          if ((Split-Path -Leaf $p) -eq 'ClaudeBoard.exe' -and (Test-Path -LiteralPath (Join-Path $pdir 'uninstall.exe') -PathType Leaf)) { $dir = $pdir }
        }
      }

      # 3) 실행 중인 보드(알림 패널 포함)를 끄고 파일을 하나씩 바꿔 끼운다 — 하나라도 실패하면 모두 되돌린다.
      # 실행 중인 exe 도 이름은 바꿀 수 있으므로 원래 파일은 .old-<시각> 으로 비켜 두었다가 끝나면 지운다
      Write-Host "3) 앱 설치: $dir"
      $procs = @(Get-Process -Name ClaudeBoard -ErrorAction SilentlyContinue | Where-Object { $_.Path })
      if ($procs.Count -gt 0) {
        $procs | Stop-Process -Force -ErrorAction SilentlyContinue
        $procs | Wait-Process -Timeout 10 -ErrorAction SilentlyContinue
      }
      New-Item -ItemType Directory -Path $dir -Force | Out-Null
      Get-ChildItem -LiteralPath $dir -Filter '*.old-*' -File -ErrorAction SilentlyContinue | Remove-Item -Force -ErrorAction SilentlyContinue
      $stamp = Get-Date -Format 'yyyyMMddHHmmss'
      $done = New-Object System.Collections.ArrayList
      try {
        foreach ($f in Get-ChildItem -LiteralPath $stage -File) {
          $dst = Join-Path $dir $f.Name
          $bak = $null
          if (Test-Path -LiteralPath $dst) {
            $bak = "$dst.old-$stamp"
            Move-Item -LiteralPath $dst -Destination $bak
          }
          [void]$done.Add(@($dst, $bak))
          Copy-Item -LiteralPath $f.FullName -Destination $dst
        }
      } catch {
        $why = $_.Exception.Message
        for ($i = $done.Count - 1; $i -ge 0; $i--) {
          $dst = $done[$i][0]; $bak = $done[$i][1]
          Remove-Item -LiteralPath $dst -Force -ErrorAction SilentlyContinue
          if ($bak) { Move-Item -LiteralPath $bak -Destination $dst -Force -ErrorAction SilentlyContinue }
        }
        throw "앱 파일을 바꿔 넣지 못해 원래대로 되돌렸습니다. 보드를 닫고 다시 실행하세요. ($why)"
      }
      foreach ($d in $done) { if ($d[1]) { Remove-Item -LiteralPath $d[1] -Force -ErrorAction SilentlyContinue } }
      Get-ChildItem -LiteralPath $dir -File | Unblock-File -ErrorAction SilentlyContinue
    } finally {
      Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue
    }

    # 4) 사용자 설정 — hook 사본·app-path·settings.json hook·claude-board 명령·로그인 자동 실행
    Write-Host '4) 사용자 설정(hook·명령·자동 실행)'
    $hook = Join-Path $dir 'claude-board-hook.exe'
    $app = Join-Path $dir 'ClaudeBoard.exe'
    # 출력이 파일로 넘어갈 때 PowerShell 5.1 이 stderr 줄을 오류로 바꿔 멈추지 않게 이 호출만 Continue 로 둔다
    $ErrorActionPreference = 'Continue'
    & $hook setup --app $app | Out-Host
    $code = $LASTEXITCODE
    $ErrorActionPreference = 'Stop'
    if ($code -ne 0) {
      Start-Process -FilePath $app -ErrorAction SilentlyContinue
      throw "사용자 설정 중 실패했습니다(종료 코드 $code). 위 메시지를 확인하고 다시 실행하세요."
    }

    # WebView2 런타임이 없으면 보드 창이 뜨지 않는다 — setup.exe 와 달리 이 스크립트는 런타임을 설치하지 않으므로 알리기만 한다
    $webview = $false
    foreach ($k in @('HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}',
                     'HKCU:\Software\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}')) {
      $pv = (Get-ItemProperty -Path $k -Name pv -ErrorAction SilentlyContinue).pv
      if ($pv -and $pv -ne '0.0.0.0') { $webview = $true }
    }
    if (-not $webview) {
      Write-Host 'WebView2 런타임을 찾지 못했습니다. 보드가 뜨지 않으면 https://developer.microsoft.com/microsoft-edge/webview2/ 에서'
      Write-Host 'Evergreen 런타임을 설치하세요(Windows 11 에는 기본으로 들어 있습니다).'
    }

    # 5) 보드 띄우기
    Write-Host '5) 보드 실행'
    Start-Process -FilePath $app

    Write-Host ''
    Write-Host '설치를 마쳤습니다.'
    if ($first) {
      Write-Host '이미 실행 중인 Claude Code 세션은 다시 시작해야 hook 이 적용됩니다.'
    }
  } finally {
    if ($null -ne $prevEncoding) { try { [Console]::OutputEncoding = $prevEncoding } catch { } }
  }
}

Install-ClaudeBoard -Version $Version -Update:$Update
