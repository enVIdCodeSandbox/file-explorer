; Run by the installer Tauri builds (bundle > windows > nsis > installerHooks).
;
; If the app was made the default file manager, folders in Windows point at
; its program file. Removing the app without undoing that would leave folders
; with nothing to open them, so the app is asked to put things back first.
!macro NSIS_HOOK_PREUNINSTALL
  IfFileExists "$INSTDIR\${MAINBINARYNAME}.exe" 0 +2
    ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --restore-file-manager'
!macroend
