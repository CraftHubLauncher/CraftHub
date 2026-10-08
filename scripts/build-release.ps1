<#
.SYNOPSIS
  Builds the CraftHub release (exe + NSIS installer) without embedding local details.

.DESCRIPTION
  Prevention happens at build time; nothing is patched or stripped afterwards, so the output
  is a normal linker product that can be Authenticode-signed later:

    * --remap-path-prefix maps the Cargo home, rustup home and repository root to neutral
      prefixes (/cargo, /rustup, /crafthub). Stable Cargo has no trim-paths yet, and Rust
      otherwise embeds dependency source paths from the user profile in panic messages.
    * The C crypto library that embedded absolute paths (aws-lc) is not used; TLS uses ring.
    * /Brepro makes the MSVC linker write deterministic PE timestamps (reproducibility).
    * strip = true / no debug info / PDB referenced by file name only (Cargo profile).
    * Cargo.lock is enforced (cargo metadata --locked) before building.
    * The frontend is built without source maps.

  Afterwards privacy-scan.ps1 -Mode Release *verifies* the result and fails the build on any
  identifying path, machine detail or secret. It only reads the files.

  Extra arguments are passed to `tauri build`, e.g.  ./scripts/build-release.ps1 --bundles nsis
#>
# Plain script (no param block) so tauri flags like --bundles are forwarded untouched via $args.
$TauriArgs = @($args | Where-Object { $_ -ne '--' })

$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE '.cargo' }
$rustupHome = if ($env:RUSTUP_HOME) { $env:RUSTUP_HOME } else { Join-Path $env:USERPROFILE '.rustup' }

# CARGO_ENCODED_RUSTFLAGS uses 0x1f as separator, so paths containing spaces stay intact.
$sep = [char]0x1f
$flags = @(
  "--remap-path-prefix=$cargoHome=/cargo",
  "--remap-path-prefix=$rustupHome=/rustup",
  "--remap-path-prefix=$repo=/crafthub",
  "-Clink-arg=/Brepro"
)
$env:CARGO_ENCODED_RUSTFLAGS = $flags -join $sep
Remove-Item Env:RUSTFLAGS -ErrorAction SilentlyContinue

Push-Location $repo
try {
  # Fail fast if Cargo.lock is not up to date: releases must build exactly the locked versions.
  $ErrorActionPreference = 'Continue'
  cargo metadata --locked --format-version 1 > $null
  if ($LASTEXITCODE -ne 0) { throw 'Cargo.lock is out of date; run cargo update deliberately and commit it.' }
  # Native tools report progress on stderr; Windows PowerShell 5.1 would turn that into
  # terminating errors under 'Stop', so judge success by the exit code instead.
  $ErrorActionPreference = 'Continue'
  npx tauri build @TauriArgs
  $code = $LASTEXITCODE
  $ErrorActionPreference = 'Stop'
  if ($code -ne 0) { throw "tauri build failed ($code)" }
} finally {
  Pop-Location
  Remove-Item Env:CARGO_ENCODED_RUSTFLAGS -ErrorAction SilentlyContinue
}

& (Join-Path $PSScriptRoot 'privacy-scan.ps1') -Mode Release
if ($LASTEXITCODE -ne 0) {
  Write-Host 'Release build contains identifying details or secrets; do not distribute it.' -ForegroundColor Red
  exit 1
}
