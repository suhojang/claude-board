# Claude Board for Windows 제거 — 설치가 넣은 hook·자동 실행·명령·저장한 계정·앱 폴더를 지운다
#   irm https://raw.githubusercontent.com/suhojang/claude-board/main/windows/packaging/uninstall.ps1 | iex
# 사용자 설정은 claude-board-hook.exe uninstall 이 되돌리고(지금 Claude Code 로그인은 그대로 둔다), 앱 폴더는 이 스크립트가 지운다
# (실행 중인 자기 exe 는 지울 수 없으므로). setup.exe 로 설치한 폴더면 그 제거 프로그램도 돌려 설정 > 앱 목록에서 뺀다.
# 앱 폴더의 자율주행 claude-autopilot.exe·claude-autopilot-engine.exe 와 plugins\md-render\ 는 앱 폴더와 함께 지워지고, 보드가 옮겨 쓴 %USERPROFILE%\.claude\notifier\plugins 도 이 스크립트가 지운다
# (macOS Uninstall.command 가 notifier 의 plugins 를 지우는 것과 같다. CLAUDE_BOARD_NOTIFIER_DIR 로 옮긴 실행에서는 plugins 를 두고 hook uninstall 도 같다).
# install.ps1 과 같이 Windows PowerShell 5.1 에서도 돌게 쓰고, exit 를 쓰지 않으며, BOM 없이 코드는 ASCII 로만 쓴다.

# 받는 도중 끊겨도 아무것도 실행되지 않게 전체를 함수로 감싸 마지막 줄에서 부른다
function Uninstall-ClaudeBoard {
  $ErrorActionPreference = 'Stop'
  $Notifier = Join-Path $env:USERPROFILE '.claude\notifier'

  #region quit-request
  # 떠 있는 보드에 정상 종료를 청한다 - macOS install.sh 의 ask_board_quit 와 같은 길이다(SPEC 5.8). ClaudeBoard.exe --action quit - 은 보드의 종료와 같은
  # 확인 창(작업 중인 보드 탭 세션이 있을 때만 뜬다)을 거치고, 끄면 끊길 세션에 일시중지 표지(.paused)를 남긴다(Stop-Process 는 확인·표지 없이 끊는다).
  # 요청을 받은 보드는 %TEMP%\claude-board.quit 에 확인 창을 띄우는 동안 asking 을, 취소하면 cancel 을 쓴다. 요청은 바꿔 끼우기 전에 지금 깔린 보드 exe 로 보낸다.
  # 돌려주는 값: none(떠 있는 보드 없음) · gone(보드가 꺼졌다) · legacy(5초 안에 표지도 종료도 없음 - 요청을 모르는 옛 보드) ·
  #              cancel(확인 창에서 취소) · timeout(기다린 시간 안에 답이 없음). 기다리는 시간은 기본 120초(CLAUDE_BOARD_INSTALL_QUIT_WAIT 초로 바꾼다)
  # CLAUDE_BOARD_TAB=1 은 보드 탭 셸의 환경 블록에 보드가 넣는다 - 이 셸에서 보드를 끄면 이 스크립트도 함께 끊긴다
  function Test-BoardTab { return ($env:CLAUDE_BOARD_TAB -eq '1') }
  # 실행 중인 보드 프로세스. -Main 이면 알림 패널(--notify)을 뺀 보드 본체만
  function Get-BoardProcs([switch]$Main) {
    $all = @(Get-Process -Name ClaudeBoard -ErrorAction SilentlyContinue | Where-Object { $_.Path })
    if (-not $Main) { return $all }
    $panels = @{}
    try {
      foreach ($c in @(Get-CimInstance -ClassName Win32_Process -Filter "Name = 'ClaudeBoard.exe'" -ErrorAction Stop)) {
        if ("$($c.CommandLine)" -match '--notify') { $panels[[int]$c.ProcessId] = $true }
      }
    } catch { }
    return @($all | Where-Object { -not $panels.ContainsKey([int]$_.Id) })
  }
  function Get-QuitMark([string]$Ack) {
    try { return ([IO.File]::ReadAllText($Ack).Trim()) } catch { return '' }
  }
  function Request-BoardQuit {
    $main = @(Get-BoardProcs -Main)
    if ($main.Count -eq 0) { return 'none' }
    $wait = 120
    $n = 0
    if ($env:CLAUDE_BOARD_INSTALL_QUIT_WAIT -and [int]::TryParse($env:CLAUDE_BOARD_INSTALL_QUIT_WAIT, [ref]$n) -and $n -gt 0) { $wait = $n }
    $ack = Join-Path ([IO.Path]::GetTempPath()) 'claude-board.quit'
    Remove-Item -LiteralPath $ack -Force -ErrorAction SilentlyContinue
    try {
      Start-Process -FilePath $main[0].Path -ArgumentList @('--action', 'quit', '-') -WindowStyle Hidden -ErrorAction Stop | Out-Null
    } catch { return 'legacy' }
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while ($sw.Elapsed.TotalSeconds -lt 5 -and -not (Test-Path -LiteralPath $ack) -and @(Get-BoardProcs -Main).Count -gt 0) { Start-Sleep -Milliseconds 100 }
    if (@(Get-BoardProcs -Main).Count -eq 0) { Remove-Item -LiteralPath $ack -Force -ErrorAction SilentlyContinue; return 'gone' }
    if (-not (Test-Path -LiteralPath $ack)) { return 'legacy' }
    Write-Host '   떠 있는 보드에 종료를 청했습니다. 작업 중인 세션이 있어 확인 창이 떴으면 답해 주세요(기다립니다)'
    while ($sw.Elapsed.TotalSeconds -lt ($wait + 5)) {
      if (@(Get-BoardProcs -Main).Count -eq 0) { Remove-Item -LiteralPath $ack -Force -ErrorAction SilentlyContinue; return 'gone' }
      if ((Get-QuitMark $ack) -eq 'cancel') { Remove-Item -LiteralPath $ack -Force -ErrorAction SilentlyContinue; return 'cancel' }
      Start-Sleep -Milliseconds 200
    }
    Remove-Item -LiteralPath $ack -Force -ErrorAction SilentlyContinue
    return 'timeout'
  }
  # 보드를 끄고 앱 파일을 바꿔도 되는지 정한다. 돌려주는 값: keep(보드를 두고 파일만 바꾼다 - 보드 탭 안) · stopped(보드가 꺼졌거나 없다)
  # 못 끄면 throw 한다(바꾼 것이 없는 채로). $Forced 는 보드가 이미 확인을 거친 길(보드의 업데이트 단추, -Update)이라 요청 없이 끈다
  function Stop-BoardForInstall([bool]$Forced, [string]$What) {
    $running = @(Get-BoardProcs)
    if ($running.Count -eq 0) { return 'stopped' }
    if (Test-BoardTab) { return 'keep' }
    if (-not $Forced) {
      $r = Request-BoardQuit
      if ($r -eq 'cancel') { throw "보드 종료를 취소해 ${What}하지 않았습니다. 바꾼 것은 없습니다. 다시 실행하면 처음부터 합니다." }
      if ($r -eq 'timeout') {
        Write-Host '   보드가 아직 켜져 있습니다. 강제로 끄면 보드 탭에서 돌던 세션이 확인·일시중지 표지 없이 끊깁니다.'
        $answer = $null
        if (-not [Console]::IsInputRedirected) {
          try { $answer = Read-Host '   강제로 끄고 계속할까요? [y/N]' } catch { $answer = $null }
        }
        if ($null -eq $answer -or $answer -notmatch '^[Yy]') {
          throw "보드가 시간 안에 꺼지지 않았고 강제로 끄지도 않아 ${What}하지 않았습니다. 바꾼 것은 없습니다. 보드를 닫고 다시 실행하세요."
        }
      }
    }
    # 남은 것(알림 패널·요청을 모르는 옛 보드·강제로 끄기로 한 보드)은 끈다
    $left = @(Get-BoardProcs)
    if ($left.Count -gt 0) {
      $left | Stop-Process -Force -ErrorAction SilentlyContinue
      $left | Wait-Process -Timeout 10 -ErrorAction SilentlyContinue
    }
    return 'stopped'
  }
  #endregion quit-request

  $prevEncoding = $null
  try { $prevEncoding = [Console]::OutputEncoding; [Console]::OutputEncoding = New-Object System.Text.UTF8Encoding $false } catch { }
  try {
    Write-Host '== Claude Board for Windows 제거 =='

    # 보드를 끄기 전에 정상 종료를 청한다 - 아래 hook uninstall 도 보드를 끄므로 그보다 먼저 한다. 보드 탭 셸 안에서는 보드를 끄는 순간
    # 탭의 세션이 확인·일시중지 표지 없이 끊기고 이 스크립트도 함께 끊겨 앱 폴더를 못 지우므로, 바꾼 것 없이 멈추고 보드 밖 PowerShell 에서 다시 하게 한다
    if ((@(Get-BoardProcs)).Count -gt 0 -and (Test-BoardTab)) {
      throw '보드 안 터미널(보드 탭)에서는 보드를 끄지 않고 제거할 수 없어 제거하지 않았습니다. 바꾼 것은 없습니다. 보드 밖 PowerShell 에서 다시 실행하세요.'
    }
    [void](Stop-BoardForInstall $false '제거')

    # 앱 폴더 — 기본 위치와, app-path 가 가리키는 폴더(setup.exe 로 설치했으면 이쪽)
    $dirs = New-Object System.Collections.ArrayList
    [void]$dirs.Add((Join-Path $env:LOCALAPPDATA 'Programs\Claude Board'))
    $appPathFile = Join-Path $Notifier 'app-path'
    if (Test-Path -LiteralPath $appPathFile -PathType Leaf) {
      $p = [IO.File]::ReadAllText($appPathFile).Trim()
      if ($p -and (Split-Path -Leaf $p) -eq 'ClaudeBoard.exe') {
        $pdir = Split-Path -Parent $p
        if (-not ($dirs -contains $pdir)) { [void]$dirs.Add($pdir) }
      }
    }

    # 0) 보드가 옮겨 쓴 md-render 사본 - claude-board-hook uninstall(crates/hook/src/setup.rs)과 같은 범위를 지운다:
    # macOS Uninstall.command 의 `rm -rf notifier/plugins` 처럼 notifier\plugins 를 통째로 지운다. 그 안에는 보드가 옮긴 다섯 파일과
    # Claude Code 가 플러그인 폴더에 만드는 tsconfig.json·.claude-plugin\types 밖에 없다(사용자가 따로 설치한 플러그인은
    # Claude Code 가 자기 plugins 폴더 %USERPROFILE%\.claude\plugins 에 둔다). 링크(junction)는 따라가지 않고 링크만 뗀다.
    # CLAUDE_BOARD_NOTIFIER_DIR 로 notifier 를 옮겨 둔 실행에서는 plugins 를 지우지 않는다(hook uninstall 도 같다).
    # hook uninstall 이 notifier 를 비었을 때만 지우므로 그보다 먼저 치운다
    #region plugins-copy
    function Test-Link([string]$Path) {
      $item = Get-Item -LiteralPath $Path -Force -ErrorAction SilentlyContinue
      return [bool]($item -and ($item.Attributes -band [IO.FileAttributes]::ReparsePoint))
    }
    function Remove-PluginsCopy([string]$NotifierDir) {
      Write-Host '0) md-render 플러그인 사본 제거'
      if ($env:CLAUDE_BOARD_NOTIFIER_DIR) {
        Write-Host '   (CLAUDE_BOARD_NOTIFIER_DIR 로 notifier 를 옮긴 실행이라 plugins 는 지우지 않습니다)'
        return
      }
      $plugins = Join-Path $NotifierDir 'plugins'
      if (-not (Test-Path -LiteralPath $plugins)) { return }
      try {
        if (Test-Link $plugins) {
          # plugins 자체가 링크면 가리키는 곳의 파일을 지우지 않게 링크만 뗀다
          [IO.Directory]::Delete($plugins)
        } else {
          # 두 단계 아래까지의 링크를 먼저 떼고(plugins\md-render, plugins\md-render\hooks 등), 나머지는 링크를 따라가지 않는 .NET 으로 지운다
          foreach ($child in @(Get-ChildItem -LiteralPath $plugins -Force -Directory)) {
            if (Test-Link $child.FullName) { [IO.Directory]::Delete($child.FullName); continue }
            foreach ($inner in @(Get-ChildItem -LiteralPath $child.FullName -Force -Directory)) {
              if (Test-Link $inner.FullName) { [IO.Directory]::Delete($inner.FullName) }
            }
          }
          [IO.Directory]::Delete($plugins, $true)
        }
        Write-Host "   $plugins"
      } catch {
        Write-Host "   ($plugins 를 지우지 못했습니다 - 보드와 Claude Code 세션을 끄고 폴더를 직접 지우세요: $($_.Exception.Message))"
      }
    }
    #endregion plugins-copy
    Remove-PluginsCopy $Notifier

    # 1) 사용자 설정 되돌리기 — 앱 폴더의 원본을 먼저, 없으면 hook 이 실행하는 사본을 쓴다
    Write-Host '1) settings.json 의 hook·자동 실행·명령·저장한 계정 제거'
    $hook = $null
    $candidates = @()
    foreach ($d in $dirs) { $candidates += (Join-Path $d 'claude-board-hook.exe') }
    $candidates += (Join-Path $Notifier 'claude-board-hook.exe')
    foreach ($c in $candidates) {
      if (Test-Path -LiteralPath $c -PathType Leaf) { $hook = $c; break }
    }
    $code = 1
    if ($hook) {
      $ErrorActionPreference = 'Continue'
      & $hook uninstall | Out-Host
      $code = $LASTEXITCODE
      $ErrorActionPreference = 'Stop'
    }
    if ($code -ne 0) {
      Write-Host '   (hook 제거 실패 — %USERPROFILE%\.claude\settings.json 에서 .claude/notifier/ 가 들어간 hook 을 직접 지우세요)'
    }

    # 2) 보드(알림 패널 포함)를 끄고 앱 폴더를 지운다
    Write-Host '2) 앱 제거'
    # (위에서 보드를 이미 껐다. 그 사이 다시 뜬 것이 있으면 앱 폴더를 지우기 전에 끈다)
    $procs = @(Get-BoardProcs)
    if ($procs.Count -gt 0) {
      $procs | Stop-Process -Force -ErrorAction SilentlyContinue
      $procs | Wait-Process -Timeout 10 -ErrorAction SilentlyContinue
    }
    foreach ($d in $dirs) {
      if (-not (Test-Path -LiteralPath $d)) { continue }
      # setup.exe 로 설치한 폴더 — NSIS 제거 프로그램은 자신을 %TEMP% 로 복사해 돌고 바로 돌아오므로 uninstall.exe 가 사라질 때까지 기다린다
      $u = Join-Path $d 'uninstall.exe'
      if (Test-Path -LiteralPath $u -PathType Leaf) {
        Start-Process -FilePath $u -ArgumentList '/S' -Wait
        for ($i = 0; $i -lt 60 -and (Test-Path -LiteralPath $u); $i++) { Start-Sleep -Milliseconds 500 }
      }
      Remove-Item -LiteralPath $d -Recurse -Force -ErrorAction SilentlyContinue
      if (Test-Path -LiteralPath $d) {
        Write-Host "   ($d 를 지우지 못했습니다 — 보드를 끄고 폴더를 직접 지우세요)"
      } else {
        Write-Host "   $d"
      }
    }

    Write-Host ''
    Write-Host '제거를 마쳤습니다. 실행 중인 Claude Code 세션은 다시 시작해야 hook 제거가 적용됩니다.'
  } finally {
    if ($null -ne $prevEncoding) { try { [Console]::OutputEncoding = $prevEncoding } catch { } }
  }
}

Uninstall-ClaudeBoard
