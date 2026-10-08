<#
.SYNOPSIS
  Regression test for a clean CI checkout without Git author configuration.
#>
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$tmp = Join-Path ([IO.Path]::GetTempPath()) ('crafthub-privacy-regression-' + [guid]::NewGuid().ToString('N'))
$oldGlobal = $env:GIT_CONFIG_GLOBAL
$oldSystem = $env:GIT_CONFIG_SYSTEM
$oldNoSystem = $env:GIT_CONFIG_NOSYSTEM

try {
  New-Item -ItemType Directory -Path (Join-Path $tmp 'scripts') -Force | Out-Null
  Copy-Item (Join-Path $repo 'scripts\privacy-scan.ps1') (Join-Path $tmp 'scripts\privacy-scan.ps1')
  Copy-Item (Join-Path $repo 'scripts\privacy-allowlist.txt') (Join-Path $tmp 'scripts\privacy-allowlist.txt')

  # The temporary copy has no .git directory and these config paths do not exist.
  # This models a fresh Actions checkout with no configured author identity.
  $env:GIT_CONFIG_GLOBAL = Join-Path $tmp 'missing-global'
  $env:GIT_CONFIG_SYSTEM = Join-Path $tmp 'missing-system'
  $env:GIT_CONFIG_NOSYSTEM = '1'

  $output = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $tmp 'scripts\privacy-scan.ps1') -Mode Repo *>&1 | Out-String
  $code = $LASTEXITCODE
  if ($code -ne 0) { throw "clean CI privacy scan returned exit code $code" }
  if ($output -notmatch 'repository privacy scan: no findings') {
    throw 'clean CI privacy scan did not report a clean result'
  }
  Write-Host 'privacy scanner regression: clean CI checkout passed with exit code 0'
}
finally {
  if ($null -eq $oldGlobal) { Remove-Item Env:GIT_CONFIG_GLOBAL -ErrorAction SilentlyContinue } else { $env:GIT_CONFIG_GLOBAL = $oldGlobal }
  if ($null -eq $oldSystem) { Remove-Item Env:GIT_CONFIG_SYSTEM -ErrorAction SilentlyContinue } else { $env:GIT_CONFIG_SYSTEM = $oldSystem }
  if ($null -eq $oldNoSystem) { Remove-Item Env:GIT_CONFIG_NOSYSTEM -ErrorAction SilentlyContinue } else { $env:GIT_CONFIG_NOSYSTEM = $oldNoSystem }
  Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
}
