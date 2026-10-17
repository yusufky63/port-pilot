; NSIS hooks for the Port Pilot installer.

; Tauri's "Delete app data" only removes `$APPDATA\${BUNDLEID}` and `$LOCALAPPDATA\${BUNDLEID}`,
; while Port Pilot keeps its settings in `$APPDATA\PortPilot` (`state::data_dir`). Same
; condition as Tauri's own removal: the box is unticked by default and never shown in silent and
; passive runs, so an upgrade keeps the settings unless the user ticks it. The portable build
; keeps its data beside the exe and has no uninstaller. The `$APPDATA` check keeps an unresolved
; shell folder from turning the path into `\PortPilot` at the root of the current drive.
!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    SetShellVarContext current
    ${If} $APPDATA != ""
      RmDir /r "$APPDATA\PortPilot"
    ${EndIf}
  ${EndIf}
!macroend
