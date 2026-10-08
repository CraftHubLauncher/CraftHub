; CraftHub desktop shortcut ownership and lifecycle.
;
; Tauri's generated installer has a desktop-shortcut path on the finish page.
; Its helper can replace a same-named, unrelated shortcut, so disable that
; path while installing and handle the desktop shortcut here. The Start-menu
; helper is restored below and remains Tauri-managed.
Var CraftHubOriginalNoShortcutMode
Var CraftHubDesktopShortcut
Var CraftHubOldMainBinaryName
Var CraftHubLegacyDesktopOwned

!macro NSIS_HOOK_PREINSTALL
  StrCpy $CraftHubOriginalNoShortcutMode $NoShortcutMode
  ReadRegStr $CraftHubOldMainBinaryName SHCTX "${UNINSTKEY}" "MainBinaryName"
  StrCpy $CraftHubLegacyDesktopOwned 0
  ReadRegStr $CraftHubDesktopShortcut SHCTX "${UNINSTKEY}" "CraftHubDesktopShortcut"
  ${If} $CraftHubDesktopShortcut == ""
    StrCpy $CraftHubDesktopShortcut "$DESKTOP\CraftHub.lnk"
  ${EndIf}
  Call CraftHubDesktopShortcutOwned
  Pop $0
  ${If} $0 = 1
    StrCpy $CraftHubLegacyDesktopOwned 1
    WriteRegStr SHCTX "${UNINSTKEY}" "CraftHubDesktopShortcut" "$CraftHubDesktopShortcut"
  ${EndIf}
  StrCpy $NoShortcutMode 1
!macroend

; Pushes 1 only when the candidate is a CraftHub shortcut. A target match is
; the ownership marker for legacy installers that predate the registry value.
Function CraftHubDesktopShortcutOwned
  IfFileExists "$CraftHubDesktopShortcut" 0 CraftHubShortcutNotOwned
  !insertmacro IsShortcutTarget "$CraftHubDesktopShortcut" "$INSTDIR\crafthub.exe"
  Pop $0
  ${If} $0 = 1
    Push 1
    Return
  ${EndIf}

  ${If} $CraftHubOldMainBinaryName != ""
    !insertmacro IsShortcutTarget "$CraftHubDesktopShortcut" "$INSTDIR\$CraftHubOldMainBinaryName"
    Pop $0
    ${If} $0 = 1
      Push 1
      Return
    ${EndIf}
  ${EndIf}

CraftHubShortcutNotOwned:
  Push 0
FunctionEnd

Function CraftHubCreateDesktopShortcut
  ReadRegStr $CraftHubDesktopShortcut SHCTX "${UNINSTKEY}" "CraftHubDesktopShortcut"
  ${If} $CraftHubLegacyDesktopOwned = 1
    Goto CraftHubRefreshDesktopShortcut
  ${EndIf}
  ${If} $CraftHubDesktopShortcut != ""
    Call CraftHubDesktopShortcutOwned
    Pop $0
    ${If} $0 = 1
      Goto CraftHubRefreshDesktopShortcut
    ${EndIf}
  ${EndIf}

  ; Adopt the legacy default only if it targets this installation.
  StrCpy $CraftHubDesktopShortcut "$DESKTOP\CraftHub.lnk"
  Call CraftHubDesktopShortcutOwned
  Pop $0
  ${If} $0 = 1
    Goto CraftHubRefreshDesktopShortcut
  ${EndIf}

  ; Never replace an unrelated shortcut. Allocate a deterministic alternate.
  IfFileExists "$CraftHubDesktopShortcut" 0 CraftHubCreateDesktopShortcutFile
  StrCpy $R0 2
CraftHubFindDesktopShortcutSlot:
  StrCpy $CraftHubDesktopShortcut "$DESKTOP\CraftHub ($R0).lnk"
  IfFileExists "$CraftHubDesktopShortcut" 0 CraftHubCreateDesktopShortcutFile
  IntOp $R0 $R0 + 1
  IntCmp $R0 1000 CraftHubNoDesktopShortcutSlot CraftHubFindDesktopShortcutSlot CraftHubNoDesktopShortcutSlot

CraftHubNoDesktopShortcutSlot:
  Return

CraftHubRefreshDesktopShortcut:
  ; Ownership was verified immediately before this delete; recreate to refresh
  ; the current executable icon on upgrades.
  Delete "$CraftHubDesktopShortcut"

CraftHubCreateDesktopShortcutFile:
  CreateShortcut "$CraftHubDesktopShortcut" "$INSTDIR\crafthub.exe" "" "$INSTDIR\crafthub.exe" 0 SW_SHOWNORMAL "" "CraftHub"
  WriteRegStr SHCTX "${UNINSTKEY}" "CraftHubDesktopShortcut" "$CraftHubDesktopShortcut"
FunctionEnd

!macro NSIS_HOOK_POSTINSTALL
  ; Restore Tauri's normal Start-menu behavior, including update semantics.
  ${If} $CraftHubOriginalNoShortcutMode = 0
    StrCpy $NoShortcutMode 0
    Call CreateOrUpdateStartMenuShortcut
  ${EndIf}
  Call CraftHubCreateDesktopShortcut
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; Delete only the recorded path, and only while it still targets CraftHub.
  ReadRegStr $CraftHubDesktopShortcut SHCTX "${UNINSTKEY}" "CraftHubDesktopShortcut"
  ReadRegStr $CraftHubOldMainBinaryName SHCTX "${UNINSTKEY}" "MainBinaryName"
  ${If} $CraftHubDesktopShortcut == ""
    ; Legacy migration: infer ownership only from the exact installed target.
    StrCpy $CraftHubDesktopShortcut "$DESKTOP\CraftHub.lnk"
  ${EndIf}

  Call un.CraftHubDesktopShortcutOwned
  Pop $0
  ${If} $0 = 1
    !insertmacro UnpinShortcut "$CraftHubDesktopShortcut"
    Delete "$CraftHubDesktopShortcut"
  ${EndIf}
!macroend

Function un.CraftHubDesktopShortcutOwned
  IfFileExists "$CraftHubDesktopShortcut" 0 un.CraftHubShortcutNotOwned
  !insertmacro IsShortcutTarget "$CraftHubDesktopShortcut" "$INSTDIR\crafthub.exe"
  Pop $0
  ${If} $0 = 1
    Push 1
    Return
  ${EndIf}

  ${If} $CraftHubOldMainBinaryName != ""
    !insertmacro IsShortcutTarget "$CraftHubDesktopShortcut" "$INSTDIR\$CraftHubOldMainBinaryName"
    Pop $0
    ${If} $0 = 1
      Push 1
      Return
    ${EndIf}
  ${EndIf}

un.CraftHubShortcutNotOwned:
  Push 0
FunctionEnd
