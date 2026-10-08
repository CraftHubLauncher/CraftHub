<#
.SYNOPSIS
  Static safety checks for the CraftHub NSIS desktop-shortcut hook.

.DESCRIPTION
  Verifies that shortcut creation and deletion are guarded by ownership checks,
  that the installer records the selected path, and that the generated Tauri
  desktop shortcut helper is disabled while the custom safe helper runs.
  Runtime shortcut behavior still requires a Windows installer test.
#>
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$hook = Get-Content (Join-Path $root 'src-tauri\nsis\hooks.nsh') -Raw

function Assert-Contains([string]$text, [string]$pattern, [string]$message) {
  if ($text -notmatch $pattern) { throw "installer shortcut check failed: $message" }
}

Assert-Contains $hook '!macro NSIS_HOOK_PREINSTALL' 'pre-install hook is missing'
Assert-Contains $hook 'StrCpy \$NoShortcutMode 1' 'Tauri desktop helper is not disabled'
Assert-Contains $hook 'IsShortcutTarget' 'shortcut ownership is not verified'
Assert-Contains $hook 'CraftHubDesktopShortcut' 'owned path is not recorded'
Assert-Contains $hook 'WriteRegStr.*CraftHubDesktopShortcut' 'owned path is not persisted'
Assert-Contains $hook 'Delete "\$CraftHubDesktopShortcut"' 'deletion is not limited to the recorded path'
Assert-Contains $hook '\$DESKTOP\\CraftHub \(\$R0\)\.lnk' 'collision fallback is missing'
Assert-Contains $hook '!macro NSIS_HOOK_PREUNINSTALL' 'pre-uninstall safety hook is missing'

if ($hook -match 'CreateShortcut "\$DESKTOP\\CraftHub\.lnk"') {
  throw 'installer shortcut check failed: unsafe unconditional default-path creation remains'
}
if ($hook -match 'Delete "\$DESKTOP\\CraftHub\.lnk"') {
  throw 'installer shortcut check failed: unsafe unconditional default-path deletion remains'
}

Write-Output 'installer shortcut static safety checks: passed'
