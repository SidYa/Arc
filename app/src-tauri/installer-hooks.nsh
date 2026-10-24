; Removes what Arc registered (src/shell.rs): the Explorer context menu, "Open with",
; its Default apps page and the archive types pointed to Arc.
!macro ArcRemoveExtension EXT
  DeleteRegKey HKCU "Software\Classes\SystemFileAssociations\.${EXT}\shell\Arc"
  DeleteRegValue HKCU "Software\Classes\.${EXT}\OpenWithProgids" "Arc.Archive"
  ReadRegStr $0 HKCU "Software\Classes\.${EXT}" ""
  StrCmp $0 "Arc.Archive" 0 +2
    DeleteRegValue HKCU "Software\Classes\.${EXT}" ""
!macroend

!macro NSIS_HOOK_PREUNINSTALL
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
