; Removes what Arc registered (src/shell.rs): the Explorer context menu, "Open with",
; its Default apps page and the archive types pointed to Arc.
!macro ArcRemoveExtension EXT
  DeleteRegKey HKCU "Software\Classes\SystemFileAssociations\.${EXT}\shell\Arc"
  DeleteRegValue HKCU "Software\Classes\.${EXT}\OpenWithProgids" "Arc.Archive"
  ReadRegStr $0 HKCU "Software\Classes\.${EXT}" ""
  StrCmp $0 "Arc.Archive" 0 +2
    DeleteRegValue HKCU "Software\Classes\.${EXT}" ""
!macroend

; Explorer (its COM surrogate) keeps arc_shell.dll loaded, so it can't be overwritten or deleted.
; A loaded DLL can still be renamed: move it aside, the copy left behind goes on the next run.
!macro ArcMoveShellDll
  Delete "$INSTDIR\arc_shell-old-*.dll"
  System::Call 'kernel32::GetTickCount() i .r0'
  Rename "$INSTDIR\arc_shell.dll" "$INSTDIR\arc_shell-old-$0.dll"
  Delete /REBOOTOK "$INSTDIR\arc_shell-old-$0.dll"
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro ArcMoveShellDll
!macroend

; The Windows 11 context menu: Arc's sparse package (msix/register-shell.ps1, log in
; %LOCALAPPDATA%\Arc\shell-install.log). Trusting Arc's certificate asks for administrator rights once.
!macro NSIS_HOOK_POSTINSTALL
  nsExec::ExecToLog 'powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$INSTDIR\register-shell.ps1" -Dir "$INSTDIR"'
  Pop $0
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  nsExec::ExecToLog 'powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$INSTDIR\register-shell.ps1" -Remove'
  Pop $0
  !insertmacro ArcMoveShellDll
  DeleteRegKey HKCU "Software\Classes\*\shell\Arc"
  DeleteRegKey HKCU "Software\Classes\Directory\shell\Arc"
  DeleteRegKey HKCU "Software\Classes\Arc.Archive"
  DeleteRegKey HKCU "Software\Arc"
  DeleteRegValue HKCU "Software\RegisteredApplications" "Arc"
  !insertmacro ArcRemoveExtension "zip"
  !insertmacro ArcRemoveExtension "7z"
  !insertmacro ArcRemoveExtension "rar"
  !insertmacro ArcRemoveExtension "tar"
  !insertmacro ArcRemoveExtension "gz"
  !insertmacro ArcRemoveExtension "tgz"
  !insertmacro ArcRemoveExtension "xz"
  !insertmacro ArcRemoveExtension "txz"
!macroend
