<#
.SYNOPSIS
  Regression test for a clean CI checkout without Git author configuration.
#>
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$tmp = Join-Path ([IO.Path]::GetTempPath()) ('crafthub-privacy-regression-' + [guid]::NewGuid().ToString('N'))
$scriptsTmp = Join-Path $tmp 'scripts'
$scanner = Join-Path $scriptsTmp 'privacy-scan.ps1'
$runner = if (Get-Command pwsh -ErrorAction SilentlyContinue) { (Get-Command pwsh).Source } elseif (Get-Command powershell.exe -ErrorAction SilentlyContinue) { (Get-Command powershell.exe).Source } else { throw 'PowerShell executable not found' }
$oldGlobal = $env:GIT_CONFIG_GLOBAL
$oldSystem = $env:GIT_CONFIG_SYSTEM
$oldNoSystem = $env:GIT_CONFIG_NOSYSTEM
$oldGitDir = $env:GIT_DIR
$oldUsername = $env:USERNAME
$oldUser = $env:USER
$oldComputerName = $env:COMPUTERNAME
$oldHostname = $env:HOSTNAME
$oldUserProfile = $env:USERPROFILE
$oldHome = $env:HOME

try {
  New-Item -ItemType Directory -Path $scriptsTmp -Force | Out-Null
  Copy-Item (Join-Path (Join-Path $repo 'scripts') 'privacy-scan.ps1') $scanner
  Copy-Item (Join-Path (Join-Path $repo 'scripts') 'privacy-allowlist.txt') (Join-Path $scriptsTmp 'privacy-allowlist.txt')

  # The temporary copy has no .git directory and these config paths do not exist.
  # This models a fresh Actions checkout with no configured author identity.
  $env:GIT_CONFIG_GLOBAL = Join-Path $tmp 'missing-global'
  $env:GIT_CONFIG_SYSTEM = Join-Path $tmp 'missing-system'
  $env:GIT_CONFIG_NOSYSTEM = '1'

  $output = & $runner -NoProfile -ExecutionPolicy Bypass -File $scanner -Mode Repo *>&1 | Out-String
  $code = $LASTEXITCODE
  if ($code -ne 0) { throw "clean CI privacy scan returned exit code $code" }
  if ($output -notmatch 'repository privacy scan: no findings') {
    throw 'clean CI privacy scan did not report a clean result'
  }

  # Ubuntu's shared service account is intentionally not a repository-wide word needle.
  $env:USER = 'runner'
  $env:HOME = Join-Path $tmp 'runner-home'
  $env:HOSTNAME = 'runner-host'
  $runnerOutput = & $runner -NoProfile -ExecutionPolicy Bypass -File $scanner -Mode Repo *>&1 | Out-String
  $runnerCode = $LASTEXITCODE
  if ($runnerCode -ne 0) { throw "generic Linux service-account scan returned exit code $runnerCode`n$runnerOutput" }
  if ($runnerOutput -notmatch 'repository privacy scan: no findings') {
    throw 'generic Linux service-account scan did not report a clean result'
  }

  # Model Ubuntu, where USERNAME/USERPROFILE/COMPUTERNAME are absent and USER,
  # HOME and HOSTNAME are the portable environment sources.
  Remove-Item Env:USERNAME, Env:USERPROFILE, Env:COMPUTERNAME -ErrorAction SilentlyContinue
  $env:USER = 'linux-ci-user'
  $env:HOME = Join-Path $tmp 'linux-home'
  $env:HOSTNAME = 'linux-ci-host'
  $linuxOutput = & $runner -NoProfile -ExecutionPolicy Bypass -File $scanner -SelfTest *>&1 | Out-String
  $linuxCode = $LASTEXITCODE
  if ($linuxCode -ne 0) { throw "Linux-style privacy self-test returned exit code $linuxCode" }
  if ($linuxOutput -match 'linux-ci-user|linux-ci-host|linux-home') {
    throw 'Linux-style privacy self-test leaked an environment value'
  }

  # Genuine personal identifiers remain findings: both home-path forms and a Git author
  # identity are synthetic here and are never printed by the scanner.
  $windowsPersonalPath = 'C:' + '\Users\' + 'private-person\CraftHub\source.rs'
  $unixPersonalPath = '/home/' + 'private-person/CraftHub/source.rs'
  $personalEmail = 'private.author' + '@' + 'example.invalid'
  Set-Content (Join-Path $tmp 'personal-path-fixture.txt') "$windowsPersonalPath`n$unixPersonalPath" -Encoding ascii
  $gitConfig = Join-Path $tmp 'synthetic-git-config'
  Set-Content $gitConfig "[user]`n`t email = $personalEmail" -Encoding ascii
  $env:GIT_CONFIG_GLOBAL = $gitConfig
  $env:GIT_DIR = Join-Path $tmp 'missing-git-dir'
  $personalOutput = & $runner -NoProfile -ExecutionPolicy Bypass -File $scanner -Mode Repo *>&1 | Out-String
  $personalCode = $LASTEXITCODE
  if ($personalCode -ne 1) { throw "personal-identifier regression returned exit code $personalCode" }
  foreach ($category in @('user-profile path', 'unix home path', 'git user e-mail', 'e-mail address')) {
    if ($personalOutput -notmatch [regex]::Escape($category)) {
      throw "personal-identifier regression did not detect $category"
    }
  }
  if ($personalOutput -match 'private-person|private\.author') {
    throw 'personal-identifier regression leaked a synthetic value'
  }
  Write-Host 'privacy scanner regression: clean CI and Linux-style environment passed'
}
finally {
  if ($null -eq $oldGlobal) { Remove-Item Env:GIT_CONFIG_GLOBAL -ErrorAction SilentlyContinue } else { $env:GIT_CONFIG_GLOBAL = $oldGlobal }
  if ($null -eq $oldSystem) { Remove-Item Env:GIT_CONFIG_SYSTEM -ErrorAction SilentlyContinue } else { $env:GIT_CONFIG_SYSTEM = $oldSystem }
  if ($null -eq $oldNoSystem) { Remove-Item Env:GIT_CONFIG_NOSYSTEM -ErrorAction SilentlyContinue } else { $env:GIT_CONFIG_NOSYSTEM = $oldNoSystem }
  if ($null -eq $oldGitDir) { Remove-Item Env:GIT_DIR -ErrorAction SilentlyContinue } else { $env:GIT_DIR = $oldGitDir }
  if ($null -eq $oldUsername) { Remove-Item Env:USERNAME -ErrorAction SilentlyContinue } else { $env:USERNAME = $oldUsername }
  if ($null -eq $oldUser) { Remove-Item Env:USER -ErrorAction SilentlyContinue } else { $env:USER = $oldUser }
  if ($null -eq $oldComputerName) { Remove-Item Env:COMPUTERNAME -ErrorAction SilentlyContinue } else { $env:COMPUTERNAME = $oldComputerName }
  if ($null -eq $oldHostname) { Remove-Item Env:HOSTNAME -ErrorAction SilentlyContinue } else { $env:HOSTNAME = $oldHostname }
  if ($null -eq $oldUserProfile) { Remove-Item Env:USERPROFILE -ErrorAction SilentlyContinue } else { $env:USERPROFILE = $oldUserProfile }
  if ($null -eq $oldHome) { Remove-Item Env:HOME -ErrorAction SilentlyContinue } else { $env:HOME = $oldHome }
  Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
}

# The personal-identifier case intentionally ran the scanner expecting exit code 1.
# Do not let that validated native/script exit code become this test's result.
exit 0
