<#!
.SYNOPSIS
  Privacy-scans already-built Linux AppImage and Debian packages.

.DESCRIPTION
  Extracts package contents into a private temporary directory, scans the AppImage
  ELF runtime separately from its SquashFS payload, scans Debian contents, and
  inspects nested ZIP archives with bounded, traversal-safe reads. Output contains
  only package-relative names, finding categories, and counts.
#>
param(
  [string]$AppImagePath,
  [string]$DebPath,
  [switch]$SelfTest
)

$ErrorActionPreference = 'Stop'
try {
  Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem
} catch { }
$MaxFiles = 20000
$MaxTotalBytes = 1GB
$MaxFileBytes = 512MB
$MaxArchiveDepth = 3

$Generic = [ordered]@{
  'user-profile path' = '[A-Za-z]:\\Users\\(?!Public\\|Default\\|<)[^\\\x00"<>|*?\r\n]{1,64}\\'
  'cargo/rustup home path' = '\\\.(?:cargo|rustup)\\'
  'unix home path' = '/(?:home|Users)/(?!runner/|<)[a-z_][a-z0-9_.-]{1,31}/'
  'absolute PDB path' = 'RSDS[\s\S]{20}[A-Za-z]:\\'
}
$EmailPattern = '[A-Za-z0-9._%+-]{2,64}@[A-Za-z0-9-]{1,63}(?:\.[A-Za-z0-9-]{1,63}){1,3}\b'
$AllowlistPath = Join-Path $PSScriptRoot 'privacy-allowlist.txt'
$AllowlistedEmails = if (Test-Path $AllowlistPath) {
  @(Get-Content $AllowlistPath | Where-Object { $_ -and -not $_.StartsWith('#') } | ForEach-Object { $_.Trim().ToLowerInvariant() })
} else { @() }
$script:FindingCount = 0
$script:SelfTestQuiet = $false

function Write-Finding([string]$label, [string]$category, [int]$count) {
  if ($count -gt 0) {
    $script:FindingCount += 1
    if (-not $script:SelfTestQuiet) { Write-Host "FAIL $label : $category ($count)" -ForegroundColor Red }
  }
}

function Get-Needles {
  $n = [ordered]@{}
  $user = if ($env:USERNAME) { $env:USERNAME } else { $env:USER }
  $hostName = if ($env:COMPUTERNAME) { $env:COMPUTERNAME } else { $env:HOSTNAME }
  $home = if ($env:USERPROFILE) { $env:USERPROFILE } else { $env:HOME }
  if ($user -and $user.Length -ge 3 -and $user.ToLowerInvariant() -ne 'runner') { $n['this user name'] = $user }
  if ($hostName -and $hostName.Length -ge 3) { $n['this computer name'] = $hostName }
  if ($home) { $n['this profile path'] = $home }
  try {
    $mail = (& git config --get user.email 2>$null)
    if ($LASTEXITCODE -eq 0 -and $mail -and $mail.Trim().Length -ge 5) { $n['git user e-mail'] = $mail.Trim() }
  } catch { }
  $global:LASTEXITCODE = 0
  return $n
}

function Find-Bytes([byte[]]$bytes, [string]$label, $needles) {
  $texts = @([Text.Encoding]::GetEncoding(28591).GetString($bytes), [Text.Encoding]::Unicode.GetString($bytes))
  foreach ($text in $texts) {
    foreach ($key in $needles.Keys) {
      Write-Finding $label $key ([regex]::Matches($text, [regex]::Escape($needles[$key]), 'IgnoreCase').Count)
    }
    foreach ($key in $Generic.Keys) {
      Write-Finding $label $key ([regex]::Matches($text, $Generic[$key], 'IgnoreCase').Count)
    }
    $emails = @([regex]::Matches($text, $EmailPattern) | ForEach-Object { $_.Value.ToLowerInvariant() } | Where-Object { $AllowlistedEmails -notcontains $_ })
    Write-Finding $label 'e-mail address' $emails.Count
  }
}

function Read-BoundedFile([string]$path) {
  $item = Get-Item -LiteralPath $path -Force
  if ($item.Length -gt $MaxFileBytes) { throw 'file exceeds inspection size limit' }
  return [IO.File]::ReadAllBytes($path)
}

function Assert-SafeRelativePath([string]$name) {
  if ([string]::IsNullOrWhiteSpace($name) -or $name.StartsWith('/') -or $name.StartsWith('\') -or $name -match '^[A-Za-z]:') {
    throw 'unsafe archive path'
  }
  $parts = $name.Replace('\', '/') -split '/'
  if ($parts -contains '..' -or $parts -contains '') { throw 'unsafe archive path' }
}

function Get-ArchiveKind([string]$name) {
  $lower = $name.ToLowerInvariant()
  if ($lower -match '\.zip$') { return 'zip' }
  if ($lower -match '\.(tar|tgz|tar\.gz|tar\.xz|tar\.bz2)$') { return 'unsupported-tar' }
  return $null
}

function Scan-ZipStream([IO.Stream]$stream, [string]$label, $needles, [int]$depth) {
  if ($depth -gt $MaxArchiveDepth) { throw 'nested archive depth limit exceeded' }
  $zip = New-Object IO.Compression.ZipArchive($stream, [IO.Compression.ZipArchiveMode]::Read, $true)
  try {
    if ($zip.Entries.Count -gt $MaxFiles) { throw 'archive entry limit exceeded' }
    foreach ($entry in $zip.Entries) {
      Assert-SafeRelativePath $entry.FullName
      if ($entry.Length -gt $MaxFileBytes) { throw 'archive entry exceeds size limit' }
      if ($entry.FullName.EndsWith('/')) { continue }
      $memory = New-Object IO.MemoryStream
      try {
        $entry.Open().CopyTo($memory)
        $data = $memory.ToArray()
      } finally { $memory.Dispose() }
      $entryLabel = "$label/$($entry.FullName.Replace('\', '/'))"
      Find-Bytes $data $entryLabel $needles
      if ((Get-ArchiveKind $entry.FullName) -eq 'zip') {
        $nested = New-Object IO.MemoryStream(,$data)
        try { Scan-ZipStream $nested $entryLabel $needles ($depth + 1) } finally { $nested.Dispose() }
      } elseif ((Get-ArchiveKind $entry.FullName) -eq 'unsupported-tar') {
        throw 'nested tar archive cannot be safely inspected'
      }
    }
  } finally { $zip.Dispose() }
}

function Scan-File([string]$path, [string]$label, $needles) {
  $data = Read-BoundedFile $path
  Find-Bytes $data $label $needles
  $kind = Get-ArchiveKind $path
  if ($kind -eq 'zip') {
    $stream = New-Object IO.MemoryStream(,$data)
    try { Scan-ZipStream $stream $label $needles 1 } finally { $stream.Dispose() }
  } elseif ($kind -eq 'unsupported-tar') {
    throw 'nested tar archive cannot be safely inspected'
  }
}

function Scan-Tree([string]$root, [string]$label, $needles) {
  if (-not (Test-Path -LiteralPath $root -PathType Container)) { throw 'extracted package root missing' }
  foreach ($entry in @(Get-ChildItem -LiteralPath $root -Recurse -Force)) {
    if ($entry.Attributes -band [IO.FileAttributes]::ReparsePoint -or $entry.LinkType) { throw 'symlink or reparse point in extracted package' }
  }
  $files = @(Get-ChildItem -LiteralPath $root -Recurse -File -Force)
  if ($files.Count -gt $MaxFiles) { throw 'extracted file limit exceeded' }
  $total = [int64]0
  foreach ($item in $files) {
    if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint -or $item.LinkType) { throw 'symlink or reparse point in extracted package' }
    $total += $item.Length
    if ($total -gt $MaxTotalBytes) { throw 'extracted package size limit exceeded' }
    $relative = $item.FullName.Substring($root.Length).TrimStart('\', '/')
    Scan-File $item.FullName "$label/$($relative.Replace('\', '/'))" $needles
  }
}

function Assert-RegularFile([string]$path, [string]$expectedExtension) {
  if (-not (Test-Path -LiteralPath $path -PathType Leaf) -or (Get-Item -LiteralPath $path).LinkType) { throw 'package is missing or is a link' }
  if ([IO.Path]::GetExtension($path).ToLowerInvariant() -ne $expectedExtension) { throw 'unexpected package type' }
}

function Invoke-AppImageInspection([string]$path, $needles, [string]$root) {
  Assert-RegularFile $path '.appimage'
  $offsetText = (& $path --appimage-offset 2>$null | Out-String).Trim()
  if ($LASTEXITCODE -ne 0 -or $offsetText -notmatch '^\d+$') { throw 'AppImage runtime offset unavailable' }
  $offset = [int64]$offsetText
  if ($offset -le 0 -or $offset -gt $MaxFileBytes) { throw 'AppImage runtime offset is unsafe' }
  $runtime = Join-Path $root 'AppImage-runtime.elf'
  $input = [IO.File]::OpenRead($path); $output = [IO.File]::Create($runtime)
  try {
    $remaining = $offset; $buffer = New-Object byte[] 1048576
    while ($remaining -gt 0) { $read = $input.Read($buffer, 0, [Math]::Min($buffer.Length, $remaining)); if ($read -le 0) { throw 'truncated AppImage runtime' }; $output.Write($buffer, 0, $read); $remaining -= $read }
  } finally { $input.Dispose(); $output.Dispose() }
  Scan-File $runtime 'AppImage runtime' $needles

  $extract = Join-Path $root 'appimage-extract'; New-Item -ItemType Directory -Path $extract -Force | Out-Null
  Push-Location $extract
  try { & $path --appimage-extract *> $null; if ($LASTEXITCODE -ne 0) { throw 'AppImage extraction failed' } }
  finally { Pop-Location }
  Scan-Tree (Join-Path $extract 'squashfs-root') 'AppImage payload' $needles
}

function Invoke-DebInspection([string]$path, $needles, [string]$root) {
  Assert-RegularFile $path '.deb'
  if (-not (Get-Command dpkg-deb -ErrorAction SilentlyContinue)) { throw 'dpkg-deb is unavailable' }
  $extract = Join-Path $root 'deb-extract'; New-Item -ItemType Directory -Path $extract -Force | Out-Null
  & dpkg-deb -x $path $extract *> $null
  if ($LASTEXITCODE -ne 0) { throw 'DEB extraction failed' }
  Scan-Tree $extract 'DEB payload' $needles
}

if ($SelfTest) {
  $needles = [ordered]@{ 'synthetic username' = 'private-selftest-user' }
  $script:FindingCount = 0
  $script:SelfTestQuiet = $true
  $winFixture = 'C:' + '\Users\' + 'private-selftest-user\x'
  $unixFixture = '/home/' + 'private-selftest-user/x'
  $mailFixture = 'synthetic.person' + '@' + 'example.invalid'
  Find-Bytes ([Text.Encoding]::ASCII.GetBytes("$winFixture $unixFixture $mailFixture")) 'synthetic-file' $needles
  if ($script:FindingCount -lt 3) { throw 'package scanner self-test did not detect synthetic findings' }
  $cleanBefore = $script:FindingCount
  Find-Bytes ([Text.Encoding]::ASCII.GetBytes('ordinary text only')) 'clean-file' @{} 
  if ($script:FindingCount -ne $cleanBefore) { throw 'package scanner self-test found a false positive' }
  $badZip = New-Object IO.MemoryStream
  $zipOut = New-Object IO.Compression.ZipArchive($badZip, [IO.Compression.ZipArchiveMode]::Create, $true)
  $null = $zipOut.CreateEntry('../escape.txt'); $zipOut.Dispose(); $badZip.Position = 0
  try { Scan-ZipStream $badZip 'traversal-fixture' @{} 1; throw 'archive traversal was accepted' } catch { if ($_.Exception.Message -notmatch 'unsafe archive path') { throw } }
  $badZip.Dispose()
  $script:SelfTestQuiet = $false
  Write-Host 'Linux package scanner self-test: passed'
  exit 0
}

if (-not $AppImagePath -or -not $DebPath) { Write-Host 'AppImage and DEB paths are required'; exit 2 }
$temp = Join-Path ([IO.Path]::GetTempPath()) ('crafthub-linux-scan-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $temp -Force | Out-Null
try {
  $needles = Get-Needles
  Invoke-AppImageInspection $AppImagePath $needles $temp
  Invoke-DebInspection $DebPath $needles $temp
  if ($script:FindingCount) { exit 1 }
  Write-Host 'Linux package privacy scan: AppImage runtime, AppImage payload, and DEB payload passed'
  exit 0
} catch {
  Write-Host 'Linux package privacy scan failed: package extraction or inspection error' -ForegroundColor Red
  exit 1
} finally {
  Remove-Item -LiteralPath $temp -Recurse -Force -ErrorAction SilentlyContinue
}
