<#
.SYNOPSIS
  Compatibility wrapper: the release privacy check now lives in privacy-scan.ps1.
#>
& (Join-Path $PSScriptRoot 'privacy-scan.ps1') -Mode Release
exit $LASTEXITCODE
