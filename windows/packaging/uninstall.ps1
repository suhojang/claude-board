# Claude Board for Windows 제거 — 설치가 넣은 hook·자동 실행·명령·저장한 계정·앱 폴더를 지운다
#   irm https://raw.githubusercontent.com/suhojang/claude-board/main/windows/packaging/uninstall.ps1 | iex
# 사용자 설정은 claude-board-hook.exe uninstall 이 되돌리고(지금 Claude Code 로그인은 그대로 둔다), 앱 폴더는 이 스크립트가 지운다
# (실행 중인 자기 exe 는 지울 수 없으므로). setup.exe 로 설치한 폴더면 그 제거 프로그램도 돌려 설정 > 앱 목록에서 뺀다.
# 앱 폴더의 plugins\md-render\ 는 앱 폴더와 함께 지워지고, 보드가 옮겨 쓴 사본 %USERPROFILE%\.claude\notifier\plugins\md-render 도 이 스크립트가 지운다
# (macOS Uninstall.command 가 notifier 의 plugins 를 지우는 것과 같다).
# install.ps1 과 같이 Windows PowerShell 5.1 에서도 돌게 쓰고, exit 를 쓰지 않으며, BOM 없이 코드는 ASCII 로만 쓴다.

# 받는 도중 끊겨도 아무것도 실행되지 않게 전체를 함수로 감싸 마지막 줄에서 부른다
function Uninstall-ClaudeBoard {
  $ErrorActionPreference = 'Stop'
  $Notifier = Join-Path $env:USERPROFILE '.claude\notifier'

  $prevEncoding = $null
  try { $prevEncoding = [Console]::OutputEncoding; [Console]::OutputEncoding = New-Object System.Text.UTF8Encoding $false } catch { }
  try {
    Write-Host '== Claude Board for Windows 제거 =='

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

    # 0) 보드가 옮겨 쓴 md-render 사본 - 보드가 쓰는 자리는 notifier\plugins\md-render 하나뿐이고 그 안에는 보드가 옮긴 다섯 파일과
    # Claude Code 가 플러그인 폴더에 만드는 tsconfig.json·.claude-plugin\types 밖에 없다. 그래서 이 폴더는 경로만 보고 통째로 지운다.
    # 사용자가 따로 설치한 플러그인은 Claude Code 가 자기 plugins 폴더(%USERPROFILE%\.claude\plugins)에 두므로 여기에 오지 않는다.
    # hook uninstall 이 notifier 를 비었을 때만 지우므로 그보다 먼저 치운다. 상위 plugins 는 비었을 때만 지운다
    Write-Host '0) md-render 플러그인 사본 제거'
    $pluginCopy = Join-Path $Notifier 'plugins\md-render'
    if (Test-Path -LiteralPath $pluginCopy) {
      try {
        $item = Get-Item -LiteralPath $pluginCopy -Force
        if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) {
          # 링크(junction)면 가리키는 곳의 파일을 지우지 않게 링크만 뗀다
          [IO.Directory]::Delete($pluginCopy)
        } else {
          Remove-Item -LiteralPath $pluginCopy -Recurse -Force
        }
        $pluginParent = Split-Path -Parent $pluginCopy
        if ((Test-Path -LiteralPath $pluginParent -PathType Container) -and -not (Get-ChildItem -LiteralPath $pluginParent -Force)) {
          Remove-Item -LiteralPath $pluginParent -Force
        }
        Write-Host "   $pluginCopy"
      } catch {
        Write-Host "   ($pluginCopy 를 지우지 못했습니다 - 보드와 Claude Code 세션을 끄고 폴더를 직접 지우세요: $($_.Exception.Message))"
      }
    }

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
    $procs = @(Get-Process -Name ClaudeBoard -ErrorAction SilentlyContinue | Where-Object { $_.Path })
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
