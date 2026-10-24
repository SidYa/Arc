; Removes the Explorer context menu and "Open with" entries Arc registered (src/shell.rs).
!macro NSIS_HOOK_PREUNINSTALL
  DeleteRegKey HKCU "Software\Classes\*\shell\Arc"
  DeleteRegKey HKCU "Software\Classes\Directory\shell\Arc"
  DeleteRegKey HKCU "Software\Classes\Arc.Archive"
  !insertmacro ArcRemoveExtension "zip"
  !insertmacro ArcRemoveExtension "7z"
  !insertmacro ArcRemoveExtension "rar"
  !insertmacro ArcRemoveExtension "tar"
  !insertmacro ArcRemoveExtension "gz"
  !insertmacro ArcRemoveExtension "tgz"
  !insertmacro ArcRemoveExtension "xz"
  !insertmacro ArcRemoveExtension "txz"
!macroend

!macro ArcRemoveExtension EXT
  DeleteRegKey HKCU "Software\Classes\SystemFileAssociations\.${EXT}\shell\Arc"
  DeleteRegValue HKCU "Software\Classes\.${EXT}\OpenWithProgids" "Arc.Archive"
!macroend
