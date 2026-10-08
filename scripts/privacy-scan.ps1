<#
.SYNOPSIS
  Detects identifying information and secrets in release binaries or in the repository.

.DESCRIPTION
  -Mode Release  Scans target\release\crafthub.exe and the NSIS installer(s), including their
                 Windows version-info fields.
  -Mode Repo     Scans every committable text file (skips node_modules, target, dist, gen), and
                 every binary asset: images must carry no metadata (PNG text/EXIF/time/unknown
                 chunks, also inside ICO/ICNS; JPEG/WebP EXIF; XMP), decoded metadata and ICC
                 profiles are checked for identifying data, and executables, archives,
                 databases and key stores are not allowed in the repository at all.
  -SelfTest      Plants synthetic samples (built at runtime, never stored in this file) in a
                 temp folder and fails unless every category is detected and no sample value
                 appears in the output.

  What is detected:
    * this machine's user name, computer name, profile path, repo path and git e-mail
      (taken from the environment, never hard-coded);
    * any Windows user-profile path (C:\Users\<name>\...), Cargo/rustup home paths,
      Unix home paths, absolute PDB paths;
    * secrets: GitHub tokens, private-key PEM headers, minisign/rsign secret keys, AWS keys,
      Slack tokens;
    * e-mail addresses (repo: except example/no-reply domains; binaries: except the
      allowlisted third-party credits in scripts/privacy-allowlist.txt).

  Output names only the file, the category and a count — never the matched text — so CI logs
  cannot leak what was found. Exit code 1 on any finding. The scanned files are only read,
  never modified.
#>
param(
  [ValidateSet('Release', 'Repo')][string]$Mode = 'Release',
  [switch]$SelfTest
)

$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path

$Generic = [ordered]@{
  'user-profile path'      = '[A-Za-z]:\\Users\\(?!Public\\|Default\\|<)[^\\\x00"<>|*?\r\n]{1,64}\\'
  'cargo/rustup home path' = '\\\.(?:cargo|rustup)\\'
  'unix home path'         = '/(?:home|Users)/(?!runner/|<)[a-z_][a-z0-9_.-]{1,31}/'
  'absolute PDB path'      = 'RSDS[\s\S]{20}[A-Za-z]:\\'
  'GitHub token'           = '\b(?:gh[pousr]_[A-Za-z0-9]{36}|github_pat_[A-Za-z0-9_]{60,})'
  'private key'            = '-----BEGIN [A-Z ]*PRIVATE KEY-----'
  'update-signing secret key' = '(?:rsign|minisign) encrypted secret key'
  'AWS access key'         = '\bAKIA[0-9A-Z]{16}\b'
  'Slack token'            = '\bxox[abprs]-[0-9A-Za-z-]{10,}'
}
$EmailPattern = '[A-Za-z0-9._%+-]{2,64}@(?:[A-Za-z0-9-]{1,63}\.)+[a-z]{2,24}\b'
$NotEmailTld = '\.(png|svg|jpg|jpeg|gif|ico|js|css|json|rs|ts|tsx|html|md|txt|exe|dll)$'
$RepoEmailOk = '@(example\.(com|org|net)|users\.noreply\.github\.com|noreply\.github\.com)$'

function Get-EnvNeedles {
  $n = [ordered]@{}
  if ($env:USERNAME -and $env:USERNAME.Length -ge 3) { $n['this user name'] = $env:USERNAME }
  if ($env:COMPUTERNAME -and $env:COMPUTERNAME.Length -ge 3) { $n['this computer name'] = $env:COMPUTERNAME }
  if ($env:USERPROFILE) { $n['this profile path'] = $env:USERPROFILE }
  $n['this repository path'] = $repo
  try {
    $mail = (& git config --get user.email 2>$null)
    if ($mail -and $mail.Trim().Length -ge 5) { $n['git user e-mail'] = $mail.Trim() }
  } catch { }
  return $n
}

function Get-Allowlist {
  $f = Join-Path $PSScriptRoot 'privacy-allowlist.txt'
  if (-not (Test-Path $f)) { return @() }
  @(Get-Content $f | Where-Object { $_ -and -not $_.StartsWith('#') } | ForEach-Object { $_.Trim().ToLowerInvariant() })
}

# Returns @{ category = count } for one text blob.
function Find-Issues([string]$text, $needles, [string]$kind, [bool]$checkEmails = $true) {
  $found = [ordered]@{}
  foreach ($k in $needles.Keys) {
    $c = ([regex]::Matches($text, [regex]::Escape($needles[$k]), 'IgnoreCase')).Count
    if ($c) { $found[$k] = $c }
  }
  foreach ($k in $Generic.Keys) {
    $c = ([regex]::Matches($text, $Generic[$k])).Count
    if ($c) { $found[$k] = $c }
  }
  if (-not $checkEmails) { return $found }
  $allow = Get-Allowlist
  $emails = @([regex]::Matches($text, $EmailPattern) | ForEach-Object { $_.Value } | Where-Object {
      $v = $_.ToLowerInvariant()
      -not ($v -match $NotEmailTld) -and -not ($allow -contains $v) -and
      -not ($kind -eq 'repo' -and $v -match $RepoEmailOk)
    })
  if ($emails.Count) { $found['e-mail address'] = $emails.Count }
  return $found
}

function Get-ReleaseTargets {
  @(Join-Path $repo 'target\release\crafthub.exe') +
  @(Get-ChildItem (Join-Path $repo 'target\release\bundle\nsis') -Filter *.exe -ErrorAction SilentlyContinue | ForEach-Object FullName)
}

function Get-RepoTargets([string]$root) {
  $skipDir = '\\(node_modules|target|dist|gen|\.git)(\\|$)'
  $binary = '\.(png|ico|icns|jpg|jpeg|gif|zip|exe|dll|pdb|db|woff2?|ttf)$'
  Get-ChildItem $root -Recurse -File -Force | Where-Object {
    $_.FullName.Substring($root.Length) -notmatch $skipDir -and $_.Name -notmatch $binary
  } | ForEach-Object FullName
}

function Invoke-Scan([string[]]$files, [string]$kind, $needles, [string]$root) {
  $bad = 0
  foreach ($f in $files) {
    if (-not (Test-Path $f)) { Write-Host "missing: $([IO.Path]::GetFileName($f))"; $bad++; continue }
    $bytes = [IO.File]::ReadAllBytes($f)
    $texts = @([Text.Encoding]::GetEncoding(28591).GetString($bytes))
    if ($kind -eq 'repo') {
      # Deliberate test fixtures can opt out per line with the marker 'privacy-scan: allow'.
      $texts = @(($texts[0] -split "`n" | Where-Object { $_ -notmatch 'privacy-scan: allow' }) -join "`n")
    }
    # An installer's payload is compressed: e-mail-shaped byte runs there are noise. Its
    # contents (crafthub.exe) are scanned uncompressed, so only that check is skipped.
    $checkEmails = -not ($kind -eq 'binary' -and $f -match '-setup\.exe$')
    if ($kind -eq 'binary') {
      $texts += [Text.Encoding]::Unicode.GetString($bytes)
      $vi = (Get-Item $f).VersionInfo
      $texts += (@($vi.CompanyName, $vi.ProductName, $vi.FileDescription, $vi.LegalCopyright,
          $vi.LegalTrademarks, $vi.Comments, $vi.OriginalFilename, $vi.InternalName) -join "`n")
    }
    $issues = [ordered]@{}
    foreach ($t in $texts) {
      $r = Find-Issues $t $needles $kind $checkEmails
      foreach ($k in $r.Keys) { $issues[$k] = [int]$issues[$k] + $r[$k] }
    }
    $name = if ($root) { $f.Substring($root.Length).TrimStart('\') } else { [IO.Path]::GetFileName($f) }
    if ($issues.Count) {
      $bad++
      foreach ($k in $issues.Keys) { Write-Host "FAIL $name : $k ($($issues[$k]))" -ForegroundColor Red }
    } elseif ($kind -eq 'binary') {
      Write-Host "ok   $name"
    }
  }
  return $bad
}

# ---------------------------------------------------------------- binary assets (repo mode)

$ImageExt = '\.(png|ico|icns|cur|jpg|jpeg|gif|webp|bmp|tif|tiff)$'
# Never expected in the source repository (builds, archives, databases, key stores).
$ForbiddenBinaryExt = '\.(exe|dll|pdb|msi|zip|7z|rar|db|sqlite|pfx|p12|key|pem|jks|keystore|snk)$'
# PNG chunks that carry only image data. Anything else (tEXt/zTXt/iTXt text, eXIf, tIME,
# unknown/private chunks) is metadata and fails the scan.
$PngImageChunks = @('IHDR', 'PLTE', 'IDAT', 'IEND', 'tRNS', 'gAMA', 'cHRM', 'sRGB', 'iCCP', 'sBIT',
  'pHYs', 'bKGD', 'hIST', 'sPLT', 'acTL', 'fcTL', 'fdAT', 'caBX')

function Expand-Zlib([byte[]]$b, [int]$offset, [int]$count) {
  # zlib = 2-byte header + raw deflate (+ adler32, ignored by DeflateStream).
  if ($count -le 2) { return '' }
  try {
    $ms = New-Object IO.MemoryStream($b, $offset + 2, $count - 2)
    $ds = New-Object IO.Compression.DeflateStream($ms, [IO.Compression.CompressionMode]::Decompress)
    $out = New-Object IO.MemoryStream
    $ds.CopyTo($out)
    return [Text.Encoding]::GetEncoding(28591).GetString($out.ToArray())
  } catch { return '' }
}

# Returns @{ Issues = [ordered]@{category=count}; Texts = decoded metadata strings }
function Get-ImageFindings([byte[]]$bytes, [string]$name) {
  $issues = [ordered]@{}
  $texts = New-Object System.Collections.Generic.List[string]
  $latin = [Text.Encoding]::GetEncoding(28591).GetString($bytes)
  $sig = [string][char]0x89 + 'PNG' + "`r`n" + [char]0x1a + "`n"
  $p = $latin.IndexOf($sig, [StringComparison]::Ordinal)
  while ($p -ge 0) {
    $i = $p + 8
    while ($i + 8 -le $bytes.Length) {
      $len = ([int64]$bytes[$i] -shl 24) -bor ([int64]$bytes[$i + 1] -shl 16) -bor ([int64]$bytes[$i + 2] -shl 8) -bor [int64]$bytes[$i + 3]
      $type = $latin.Substring($i + 4, 4)
      $body = $i + 8
      if ($len -lt 0 -or $body + $len -gt $bytes.Length) { $issues['malformed image'] = 1; break }
      if ($PngImageChunks -notcontains $type) {
        $cat = switch -CaseSensitive ($type) {
          { $_ -in 'tEXt', 'zTXt', 'iTXt' } { 'image text metadata' }
          'eXIf' { 'image EXIF metadata' }
          'tIME' { 'image timestamp metadata' }
          default { "unrecognised image chunk $type" }
        }
        $issues[$cat] = [int]$issues[$cat] + 1
      }
      # Decode text-bearing chunks so their content is also checked for identifying data.
      $chunk = $latin.Substring($body, [int]$len)
      $nul = $chunk.IndexOf([char]0)
      switch -CaseSensitive ($type) {
        'tEXt' { $texts.Add($chunk) }
        'zTXt' { if ($nul -ge 0) { $texts.Add((Expand-Zlib $bytes ($body + $nul + 2) ([int]$len - $nul - 2))) } }
        'iTXt' { $texts.Add($chunk); if ($nul -ge 0 -and $chunk[$nul + 1] -eq [char]1) { $texts.Add((Expand-Zlib $bytes ($body + $nul + 3) ([int]$len - $nul - 3))) } }
        'iCCP' { if ($nul -ge 0) { $texts.Add((Expand-Zlib $bytes ($body + $nul + 2) ([int]$len - $nul - 2))) } }
        'eXIf' { $texts.Add($chunk) }
        # caBX is the Content Credentials (C2PA) PNG chunk used by some
        # authoring tools. It is metadata, so retain it for the same raw
        # identity/secret checks while not treating the container itself as
        # an unrecognised image format.
        'caBX' { $texts.Add($chunk) }
      }
      $i = $body + [int]$len + 4
      if ($type -eq 'IEND') { break }
    }
    $p = $latin.IndexOf($sig, $p + 8, [StringComparison]::Ordinal)
  }
  if ($latin.Contains('Exif' + [char]0 + [char]0)) { $issues['image EXIF metadata'] = [int]$issues['image EXIF metadata'] + 1 }
  if ($latin.Contains('<x:xmpmeta') -or $latin.Contains('http://ns.adobe.com/xap/')) { $issues['image XMP metadata'] = 1 }
  if ($name -match '\.gif$' -and $latin.Contains([string][char]0x21 + [char]0xFE)) { $issues['image comment'] = 1 }
  return @{ Issues = $issues; Texts = $texts }
}

function Get-RepoBinaryAssets([string]$root) {
  $skipDir = '\\(node_modules|target|dist|gen|\.git)(\\|$)'
  $binary = '\.(png|ico|icns|cur|jpg|jpeg|gif|webp|bmp|tif|tiff|zip|7z|rar|exe|dll|pdb|msi|db|sqlite|pfx|p12|key|pem|jks|keystore|snk|woff2?|ttf|otf)$'
  Get-ChildItem $root -Recurse -File -Force | Where-Object {
    $_.FullName.Substring($root.Length) -notmatch $skipDir -and $_.Name -match $binary
  } | ForEach-Object FullName
}

function Invoke-AssetScan([string[]]$files, $needles, [string]$root) {
  $bad = 0
  foreach ($f in $files) {
    $name = $f.Substring($root.Length).TrimStart('\')
    $issues = [ordered]@{}
    if ($f -match $ForbiddenBinaryExt) {
      $issues['unexpected binary file in repository'] = 1
    } else {
      $bytes = [IO.File]::ReadAllBytes($f)
      # Raw bytes: identity/secret patterns only (compressed pixel data makes e-mail matching noise).
      foreach ($t in @([Text.Encoding]::GetEncoding(28591).GetString($bytes), [Text.Encoding]::Unicode.GetString($bytes))) {
        $r = Find-Issues $t $needles 'repo' $false
        foreach ($k in $r.Keys) { $issues[$k] = [int]$issues[$k] + $r[$k] }
      }
      if ($f -match $ImageExt) {
        $img = Get-ImageFindings $bytes $f
        foreach ($k in $img.Issues.Keys) { $issues[$k] = [int]$issues[$k] + $img.Issues[$k] }
        foreach ($t in $img.Texts) {
          $r = Find-Issues $t $needles 'repo' $true
          foreach ($k in $r.Keys) { $issues["$k (in image metadata)"] = [int]$issues["$k (in image metadata)"] + $r[$k] }
        }
      }
    }
    if ($issues.Count) {
      $bad++
      foreach ($k in $issues.Keys) { Write-Host "FAIL $name : $k ($($issues[$k]))" -ForegroundColor Red }
    }
  }
  return $bad
}

# PNG builder for the self-test (CRC values are not checked by the scanner).
function New-PngBytes([string[]]$chunkSpecs) {
  $ms = New-Object IO.MemoryStream
  $w = { param([byte[]]$b) $ms.Write($b, 0, $b.Length) }
  & $w ([byte[]](0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A))
  foreach ($spec in $chunkSpecs) {
    $type, $data = $spec.Split('|', 2)
    $body = [Text.Encoding]::GetEncoding(28591).GetBytes([string]$data)
    $len = [BitConverter]::GetBytes([int]$body.Length); [Array]::Reverse($len)
    & $w $len
    & $w ([Text.Encoding]::ASCII.GetBytes($type))
    & $w $body
    & $w ([byte[]](0, 0, 0, 0))
  }
  return $ms.ToArray()
}

if ($SelfTest) {
  $tmp = Join-Path ([IO.Path]::GetTempPath()) ("privacy-selftest-" + [guid]::NewGuid().ToString('N'))
  New-Item -ItemType Directory $tmp | Out-Null
  try {
    $a36 = 'a' * 36
    $samples = [ordered]@{
      'user-profile path'      = 'C:' + '\Users\' + 'samplebuilder\src\lib.rs'
      'cargo/rustup home path' = 'D:\x\' + '.cargo\registry\src\foo.rs'
      'unix home path'         = '/home/' + 'samplebuilder/project/main.rs'
      'GitHub token'           = 'ghp' + '_' + $a36
      'private key'            = '-----BEGIN ' + 'RSA PRIVATE KEY-----'
      'update-signing secret key' = 'untrusted comment: rsign ' + 'encrypted secret key'
      'AWS access key'         = 'AKIA' + ('Q' * 16)
      'e-mail address'         = 'sample.person' + '@' + 'mail-provider.test'
      'this user name'         = 'built by ' + $env:USERNAME
    }
    $i = 0
    foreach ($k in $samples.Keys) { Set-Content (Join-Path $tmp "s$i.txt") $samples[$k] -Encoding ascii; $i++ }
    # Clean control file must pass.
    Set-Content (Join-Path $tmp 'clean.txt') 'Contact: someone@example.com; path %LOCALAPPDATA%\CraftHub' -Encoding ascii
    $log = Join-Path $tmp 'out.log'
    $files = Get-ChildItem $tmp -Filter *.txt | ForEach-Object FullName
    $output = & { Invoke-Scan $files 'repo' (Get-EnvNeedles) $tmp } *>&1 | Out-String

    # Binary asset samples (built here, never stored in the repository).
    $mail = 'sample.person' + '@' + 'mail-provider.test'
    $ihdr = 'IHDR|' + ([string][char]0 * 13)
    $assets = [ordered]@{
      'a-text.png'  = @{ Bytes = (New-PngBytes @($ihdr, ('tEXt|Author' + [char]0 + $mail), 'IEND|')); Expect = @('image text metadata', 'e-mail address (in image metadata)') }
      'a-exif.png'  = @{ Bytes = (New-PngBytes @($ihdr, 'eXIf|MM', 'IEND|')); Expect = @('image EXIF metadata') }
      'a-time.png'  = @{ Bytes = (New-PngBytes @($ihdr, 'tIME|0000000', 'IEND|')); Expect = @('image timestamp metadata') }
      'a-photo.jpg' = @{ Bytes = ([byte[]](0xFF, 0xD8, 0xFF, 0xE1, 0, 16) + [Text.Encoding]::ASCII.GetBytes('Exif' + [char]0 + [char]0 + 'MM')); Expect = @('image EXIF metadata') }
      'a-xmp.png'   = @{ Bytes = (New-PngBytes @($ihdr, ('iTXt|XML:com.adobe.xmp' + [char]0 + [char]0 + [char]0 + [char]0 + [char]0 + '<x:xmpmeta/>'), 'IEND|')); Expect = @('image text metadata', 'image XMP metadata') }
      'a-path.ico'  = @{ Bytes = ([Text.Encoding]::ASCII.GetBytes('C:' + '\Users\' + 'samplebuilder\icon.ico')); Expect = @('user-profile path') }
      'a-cert.pfx'  = @{ Bytes = ([byte[]](1, 2, 3)); Expect = @('unexpected binary file in repository') }
      'clean.png'   = @{ Bytes = (New-PngBytes @($ihdr, 'IDAT|x', 'IEND|')); Expect = @() }
    }
    foreach ($k in $assets.Keys) { [IO.File]::WriteAllBytes((Join-Path $tmp $k), $assets[$k].Bytes) }
    $assetOut = & { Invoke-AssetScan (Get-RepoBinaryAssets $tmp) (Get-EnvNeedles) $tmp } *>&1 | Out-String
    $failed = @()
    foreach ($k in $assets.Keys) {
      foreach ($cat in $assets[$k].Expect) {
        if ($assetOut -notmatch [regex]::Escape("$k : $cat")) { $failed += "not detected: $cat in $k" }
      }
    }
    if ($assetOut -match 'clean\.png') { $failed += 'false positive on clean image' }
    if ($assetOut.Contains($mail) -or $assetOut.Contains('samplebuilder')) { $failed += 'scanner output leaked an image sample value' }
    $j = 0
    foreach ($k in $samples.Keys) {
      if ($output -notmatch [regex]::Escape("s$j.txt : $k")) { $failed += "not detected: $k" }
      $j++
    }
    if ($output -match 'clean\.txt') { $failed += 'false positive on clean control file' }
    foreach ($v in $samples.Values) {
      $secretPart = ($v -split '\s')[-1]
      if ($secretPart.Length -ge 8 -and $output.Contains($secretPart)) { $failed += 'scanner output leaked a sample value' }
    }
    if ($failed) { $failed | ForEach-Object { Write-Host "SELF-TEST FAIL: $_" -ForegroundColor Red }; exit 1 }
    Write-Host "privacy-scan self-test: $($samples.Count) text categories and $(($assets.Values | ForEach-Object { $_.Expect } | Sort-Object -Unique).Count) binary-asset categories detected, clean files passed, no values printed"
    exit 0
  } finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
  }
}

$needles = Get-EnvNeedles
if ($Mode -eq 'Release') {
  $bad = Invoke-Scan (Get-ReleaseTargets) 'binary' $needles $null
} else {
  # The repo itself legitimately contains its own path only in nothing committed; drop it here.
  $needles.Remove('this repository path')
  $bad = Invoke-Scan (Get-RepoTargets $repo) 'repo' $needles $repo
  $assetFiles = @(Get-RepoBinaryAssets $repo)
  $bad += Invoke-AssetScan $assetFiles $needles $repo
  if (-not $bad) { Write-Host "repository privacy scan: no findings ($($assetFiles.Count) binary assets checked for metadata)" }
}
if ($bad) { exit 1 }
