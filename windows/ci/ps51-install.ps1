# Claude Board for Windows — install.ps1·uninstall.ps1 을 Windows PowerShell 5.1 에서 실제 사용자처럼 돌린다 (install-checks.ps1 의 시나리오 5)
# 이 파일은 UTF-8 BOM 으로 저장한다 — powershell.exe -File 은 BOM 없는 파일을 ANSI 로 읽어 한글 문자열이 깨진다.
# Windows PowerShell 5.1 문법만 쓴다(??·삼항·&& 금지).
#
#   powershell.exe -NoProfile -ExecutionPolicy Bypass -File ps51-install.ps1 -Mode <first|update|badsha|uninstall> -Script <install.ps1|uninstall.ps1> -Assets <릴리스 자산 폴더> -CiVersion <버전> -Result <결과 JSON>
#
# 웹 호출은 같은 이름의 함수를 전역에 먼저 정의해 가로챈다 — PowerShell 은 같은 이름이면 cmdlet 보다 함수를 먼저 고른다.
#   Invoke-RestMethod: 릴리스 목록. macOS 판(v 태그, Latest)·draft·prerelease 를 섞어 windows-v 정식 릴리스를 고르는지 본다.
#   Invoke-WebRequest: -OutFile 로 내려받기 — 릴리스 자산 폴더의 파일을 복사한다(badsha 면 .sha256 을 틀린 값으로).
# 결과(throw 여부·메시지·가로챈 호출)는 -Result 의 JSON 으로 남긴다. 스크립트가 exit 를 쓰면 결과 파일이 생기지 않는다.
param(
  [Parameter(Mandatory = $true)][ValidateSet('first', 'update', 'badsha', 'uninstall')][string]$Mode,
  [Parameter(Mandatory = $true)][string]$Script,
  [Parameter(Mandatory = $true)][string]$Assets,
  [Parameter(Mandatory = $true)][string]$CiVersion,
  [Parameter(Mandatory = $true)][string]$Result
)

[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding $false

# 가로챈 함수가 install.ps1 의 함수 안에서 불리므로 값은 전역에 둔다. 이름은 install.ps1 의 변수($Version·$Update 등)와 겹치지 않게
$global:CiCalls = New-Object System.Collections.ArrayList
$global:CiAssets = $Assets
$global:CiVersion = $CiVersion
$global:CiBadSha = ($Mode -eq 'badsha')

function global:Invoke-RestMethod {
  param([string]$Uri, [string]$UserAgent, [switch]$UseBasicParsing)
  [void]$global:CiCalls.Add("Invoke-RestMethod $Uri")
  Write-Host "   (가로챔) Invoke-RestMethod $Uri (UserAgent $UserAgent)"
  if ($Uri -ne 'https://api.github.com/repos/suhojang/claude-board/releases?per_page=100&page=1') {
    throw "예상하지 못한 Invoke-RestMethod 주소: $Uri"
  }
  # GitHub 릴리스 목록 API 처럼 최신이 앞 — macOS 판 릴리스(Latest)가 맨 앞이다
  $list = @(
    [pscustomobject]@{ tag_name = 'v99.1'; name = 'macOS (Latest)'; draft = $false; prerelease = $false },
    [pscustomobject]@{ tag_name = 'windows-v99.0'; name = 'draft'; draft = $true; prerelease = $false },
    [pscustomobject]@{ tag_name = 'windows-v98.0'; name = 'prerelease'; draft = $false; prerelease = $true },
    [pscustomobject]@{ tag_name = "windows-v$($global:CiVersion)"; name = 'Windows'; draft = $false; prerelease = $false },
    [pscustomobject]@{ tag_name = 'windows-v1.0'; name = 'older'; draft = $false; prerelease = $false }
  )
  # PowerShell 5.1 의 Invoke-RestMethod 는 JSON 배열을 한 덩어리(배열 하나)로 내보낸다
  return , $list
}

function global:Invoke-WebRequest {
  param([string]$Uri, [string]$OutFile, [string]$UserAgent, [switch]$UseBasicParsing)
  [void]$global:CiCalls.Add("Invoke-WebRequest $Uri")
  Write-Host "   (가로챔) Invoke-WebRequest $Uri -> $OutFile"
  $prefix = "https://github.com/suhojang/claude-board/releases/download/windows-v$($global:CiVersion)/"
  if (-not $Uri.StartsWith($prefix, [StringComparison]::Ordinal)) { throw "예상하지 못한 Invoke-WebRequest 주소: $Uri" }
  if (-not $OutFile) { throw "-OutFile 없이 불렸습니다: $Uri" }
  $name = $Uri.Substring($prefix.Length)
  $src = Join-Path $global:CiAssets $name
  if (-not (Test-Path -LiteralPath $src -PathType Leaf)) { throw "릴리스 자산에 없는 파일: $name" }
  if ($global:CiBadSha -and $name.EndsWith('.sha256')) {
    Write-Host '   (가로챔) .sha256 을 틀린 값으로 준다'
    [IO.File]::WriteAllText($OutFile, ('0' * 64) + "  ClaudeBoard-windows-x64.zip`n", (New-Object System.Text.UTF8Encoding $false))
    return
  }
  Copy-Item -LiteralPath $src -Destination $OutFile -Force
}

# irm 이 raw.githubusercontent.com 에서 받는 것과 같은 글(UTF-8)
$text = [IO.File]::ReadAllText($Script, [Text.Encoding]::UTF8)
$outcome = [ordered]@{
  mode = $Mode
  ps = $PSVersionTable.PSVersion.ToString()
  edition = "$($PSVersionTable.PSEdition)"
  inputRedirected = [Console]::IsInputRedirected
  threw = $false
  error = $null
  calls = @()
}
Write-Host "== Windows PowerShell $($outcome.ps) ($($outcome.edition)) · 표준 입력 리디렉션 $($outcome.inputRedirected) · $Mode =="
try {
  switch ($Mode) {
    # irm <url> | iex — 처음 설치(인자 없음)
    'first' { $text | Invoke-Expression }
    # 보드의 업데이트 단추와 같은 꼴(SPEC 5.8)에서 -Version 을 빼 릴리스 목록으로 버전을 고르게 한다
    'update' { & ([scriptblock]::Create($text)) -Update }
    # 보드의 업데이트 단추 그대로 — & { $s = irm '<url>'; & ([scriptblock]::Create($s)) -Version '<v>' -Update }
    'badsha' { & ([scriptblock]::Create($text)) -Version $CiVersion -Update }
    # irm <url> | iex
    'uninstall' { $text | Invoke-Expression }
  }
} catch {
  $outcome.threw = $true
  $outcome.error = $_.Exception.Message
  Write-Host "   throw: $($_.Exception.Message)"
}
$outcome.calls = @($global:CiCalls)
[IO.File]::WriteAllText($Result, (ConvertTo-Json -InputObject $outcome -Depth 5), (New-Object System.Text.UTF8Encoding $false))
Write-Host "== 끝 (throw $($outcome.threw)) =="
