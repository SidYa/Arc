# Arc

A minimalist archiver for Windows

- Create and extract **ZIP**, **7Z**, **TAR.GZ / TGZ**, **TAR.XZ**; extract **RAR**
- Browse an archive like a folder and open single files without extracting everything
- Password protection: AES-256 for ZIP and 7Z, optionally with encrypted file names (7Z)
- Explorer context menu like 7-Zip: *Add to archive*, *Add with password*, *Extract here*, *Extract to folder* — several selected archives go into one folder each
- Drag & drop, progress with speed and time left, cancel at any moment
- Dark and light themes, accent colors (including the Windows accent), English and Ukrainian

## Build

Requirements: Rust (MSVC toolchain), Node.js, Visual Studio Build Tools (C++ for UnRAR and liblzma).

```powershell
cd app
.\dev.ps1          # development
.\dev.ps1 -Build   # NSIS installer in app\src-tauri\target\release\bundle\nsis
```

Tests of the archive formats: `cargo test` in `app/src-tauri`.

## License

RAR extraction uses the UnRAR source code, which may be used to extract RAR archives but not to create them.
