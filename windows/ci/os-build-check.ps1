# Claude Board for Windows - install.ps1 OS build check unit test (Windows PowerShell 5.1 and pwsh)
#   powershell.exe -NoProfile -ExecutionPolicy Bypass -File os-build-check.ps1
#   pwsh -NoProfile -File os-build-check.ps1
# Pulls Get-OsBuild / Test-OsBuild out of install.ps1 with the PowerShell parser and runs them alone, so install.ps1 itself never runs.
# ASCII only: Windows PowerShell 5.1 reads a BOM-less file as ANSI.
$ErrorActionPreference = 'Stop'
$script = Join-Path (Split-Path -Parent $PSScriptRoot) 'packaging\install.ps1'
$tokens = $null; $errs = $null
# read as UTF-8 like irm | iex does (ParseFile would read the BOM-less file as ANSI under 5.1)
$text = [IO.File]::ReadAllText($script, (New-Object System.Text.UTF8Encoding $false))
$ast = [System.Management.Automation.Language.Parser]::ParseInput($text, [ref]$tokens, [ref]$errs)
if ($errs.Count -gt 0) { throw "install.ps1 parse errors: $($errs[0].Message)" }
foreach ($name in @('Get-OsBuild', 'Test-OsBuild')) {
  $fn = $ast.FindAll({ param($n) $n -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq $name }, $true) | Select-Object -First 1
  if (-not $fn) { throw "install.ps1 has no function $name" }
  . ([scriptblock]::Create($fn.Extent.Text))
}

$fail = 0
function Check($label, $actual, $expected) {
  $ok = ($actual -eq $expected)
  if (-not $ok) { $script:fail++ }
  Write-Host ("{0,-4} {1}: got {2}, want {3}" -f $(if ($ok) { 'ok' } else { 'FAIL' }), $label, $actual, $expected)
}

Write-Host ("PowerShell {0}" -f $PSVersionTable.PSVersion)
Check 'Test-OsBuild 17762 (1803)' (Test-OsBuild 17762) $false
Check 'Test-OsBuild 17763 (1809)' (Test-OsBuild 17763) $true
Check 'Test-OsBuild 22631 (11 23H2)' (Test-OsBuild 22631) $true
Check 'Test-OsBuild 9200 (a lying 6.2 host)' (Test-OsBuild 9200) $false
Check 'Test-OsBuild 0 (unknown)' (Test-OsBuild 0) $false

# real host: Get-OsBuild must equal the registry value and be a plausible build
$reg = [int](Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion').CurrentBuildNumber
$real = Get-OsBuild
Check 'Get-OsBuild == registry CurrentBuildNumber' $real $reg
Check 'Get-OsBuild >= 17763 on this host' ($real -ge 17763) $true
Write-Host ("host: Get-OsBuild={0} registry={1} OSVersion={2}" -f $real, $reg, [Environment]::OSVersion.Version)

# the guard must exist in the body and use the helper, not a literal compare against OSVersion
$body = $text
Check 'install.ps1 guards with Test-OsBuild' ($body.Contains('-not (Test-OsBuild $build)')) $true

if ($fail -gt 0) { Write-Host "$fail check(s) failed"; throw 'os-build-check failed' }
Write-Host 'all checks passed'
