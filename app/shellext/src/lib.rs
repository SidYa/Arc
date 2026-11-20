//! Arc in the Windows 11 Explorer context menu.
//!
//! The new menu only shows IExplorerCommand handlers of packaged apps: Arc's sparse MSIX package
//! (src-tauri/msix) declares this DLL as its COM server. Explorer asks it for an "Arc" item with
//! a submenu and hands over all the selected files at once; the command passes them to arc.exe
//! in a list file. What to show comes from HKCU\Software\Arc\Menu, written by arc.exe
//! (src-tauri/src/shell.rs): the exe path, translated titles, archive extensions, on/off.

#![allow(non_snake_case)]

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::io::Write as _;
use std::os::windows::process::CommandExt;

use windows::Win32::Foundation::{CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, E_FAIL, E_NOINTERFACE, E_NOTIMPL, S_FALSE, S_OK};
use windows::Win32::System::Com::{CoTaskMemFree, IBindCtx, IClassFactory, IClassFactory_Impl};
use windows::Win32::System::Ole::{IObjectWithSite, IObjectWithSite_Impl};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegGetValueW};
use windows::Win32::UI::Shell::{
    ECF_DEFAULT, ECF_HASSUBCOMMANDS, ECS_ENABLED, ECS_HIDDEN, IEnumExplorerCommand, IEnumExplorerCommand_Impl, IExplorerCommand,
    IExplorerCommand_Impl, IShellItemArray, SHStrDupW, SIGDN_FILESYSPATH,
};
use windows::core::{BOOL, GUID, HRESULT, HSTRING, IUnknown, Interface, PWSTR, Ref, Result, implement};

/// Must match the class in src-tauri/msix/AppxManifest.xml
const CLSID: GUID = GUID::from_u128(0x37B70A45_F2C2_4F46_8A59_3EE158D32A3D);

const MENU_KEY: &str = r"Software\Arc\Menu";

/// Submenu items: registry value with the title, command line flag, only for archives, English title.
/// "{}" in a title is the folder the archive goes into (`folder_name`).
const ITEMS: [(&str, &str, bool, &str); 5] = [
    ("Add", "--add", false, "Add to archive"),
    ("AddPassword", "--add-password", false, "Add to archive with password"),
    ("ExtractHere", "--extract-here", true, "Extract"),
    ("ExtractFolder", "--extract-folder", true, r#"Extract to "{}\""#),
    ("ExtractChoose", "--extract-choose", true, "Extract to…"),
];

/// Until Arc writes its own list (it runs after the installer)
const DEFAULT_EXTENSIONS: &str = "zip;7z;rar;tar;gz;tgz;xz;txz";

fn read_string(name: &str) -> Option<String> {
    let mut buf = vec![0u16; 1024];
    let mut size = (buf.len() * 2) as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &HSTRING::from(MENU_KEY),
            &HSTRING::from(name),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut c_void),
            Some(&mut size),
        )
    };
    if status.is_err() {
        return None;
    }
    let len = (size as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&buf[..len]))
}

fn enabled() -> bool {
    let mut value = 0u32;
    let mut size = 4u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &HSTRING::from(MENU_KEY),
            &HSTRING::from("Enabled"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut c_void),
            Some(&mut size),
        )
    };
    status.is_ok() && value != 0 && read_string("Exe").is_some()
}

fn to_pwstr(text: &str) -> Result<PWSTR> {
    unsafe { SHStrDupW(&HSTRING::from(text)) }
}

/// File system paths of the selected items.
fn paths(items: Ref<IShellItemArray>) -> Vec<String> {
    let Some(items) = items.as_ref() else { return Vec::new() };
    let mut out = Vec::new();
    unsafe {
        let count = items.GetCount().unwrap_or(0);
        for i in 0..count {
            let Ok(item) = items.GetItemAt(i) else { continue };
            if let Ok(name) = item.GetDisplayName(SIGDN_FILESYSPATH) {
                out.push(name.to_string().unwrap_or_default());
                CoTaskMemFree(Some(name.0 as *const c_void));
            }
        }
    }
    out.retain(|p| !p.is_empty());
    out
}

fn is_archive(path: &str, extensions: &[String]) -> bool {
    let name = path.rsplit(['\\', '/']).next().unwrap_or(path).to_lowercase();
    extensions.iter().any(|ext| name.len() > ext.len() + 1 && name.ends_with(&format!(".{ext}")))
}

/// Like 7-Zip: the folder of a single archive ("photos.tar.gz" -> "photos"), "*" for several.
/// The same name as archive_stem in src-tauri/src/archive/mod.rs.
fn folder_name(paths: &[String]) -> String {
    let [path] = paths else { return "*".into() };
    let name = path.rsplit(['\\', '/']).next().unwrap_or(path);
    let lower = name.to_lowercase();
    let mut cut = name.len();
    for ext in [".tar.gz", ".tar.xz", ".tgz", ".txz", ".tar", ".zip", ".7z", ".rar", ".gz", ".xz"] {
        if lower.ends_with(ext) {
            cut = name.len() - ext.len();
            break;
        }
    }
    let mut stem = name[..cut].to_string();
    // Multi-volume RAR: name.part01.rar
    if let Some(pos) = stem.to_lowercase().rfind(".part")
        && stem.len() > pos + 5
        && stem[pos + 5..].chars().all(|c| c.is_ascii_digit())
    {
        stem.truncate(pos);
    }
    if stem.is_empty() { "archive".into() } else { stem }
}

fn archive_extensions() -> Vec<String> {
    read_string("Extensions").unwrap_or_else(|| DEFAULT_EXTENSIONS.into()).split(';').filter(|s| !s.is_empty()).map(str::to_lowercase).collect()
}

/// The "Arc" item (`item` None) or one of its submenu items. Classic (non-agile) COM objects
/// with a site, like other Windows 11 menu handlers.
#[implement(IExplorerCommand, IObjectWithSite, Agile = false)]
struct Command {
    item: Option<usize>,
    site: RefCell<Option<IUnknown>>,
}

impl Command {
    fn new(item: Option<usize>) -> Command {
        Command { item, site: RefCell::new(None) }
    }
}

impl IObjectWithSite_Impl for Command_Impl {
    fn SetSite(&self, site: Ref<IUnknown>) -> Result<()> {
        *self.site.borrow_mut() = site.cloned();
        Ok(())
    }

    fn GetSite(&self, iid: *const GUID, site: *mut *mut c_void) -> Result<()> {
        unsafe { *site = std::ptr::null_mut() };
        match self.site.borrow().as_ref() {
            Some(s) => unsafe { s.query(iid, site).ok() },
            None => Err(E_NOINTERFACE.into()),
        }
    }
}

impl IExplorerCommand_Impl for Command_Impl {
    fn GetTitle(&self, items: Ref<IShellItemArray>) -> Result<PWSTR> {
        match self.item {
            None => to_pwstr("Arc"),
            Some(i) => {
                let title = read_string(ITEMS[i].0).unwrap_or_else(|| ITEMS[i].3.to_string());
                if title.contains("{}") {
                    to_pwstr(&title.replace("{}", &folder_name(&paths(items))))
                } else {
                    to_pwstr(&title)
                }
            }
        }
    }

    fn GetIcon(&self, _items: Ref<IShellItemArray>) -> Result<PWSTR> {
        let exe = read_string("Exe").ok_or(E_FAIL)?;
        // No quotes: the new Windows 11 menu doesn't take a quoted icon path
        to_pwstr(&format!("{exe},0"))
    }

    fn GetToolTip(&self, _items: Ref<IShellItemArray>) -> Result<PWSTR> {
        Err(E_NOTIMPL.into())
    }

    fn GetCanonicalName(&self) -> Result<GUID> {
        Ok(GUID::zeroed())
    }

    fn GetState(&self, items: Ref<IShellItemArray>, _slow: BOOL) -> Result<u32> {
        self.state(items)
    }

    fn Invoke(&self, items: Ref<IShellItemArray>, _bind: Ref<IBindCtx>) -> Result<()> {
        self.invoke(items)
    }

    fn GetFlags(&self) -> Result<u32> {
        Ok(if self.item.is_none() { ECF_HASSUBCOMMANDS.0 as u32 } else { ECF_DEFAULT.0 as u32 })
    }

    fn EnumSubCommands(&self) -> Result<IEnumExplorerCommand> {
        if self.item.is_some() {
            return Err(E_NOTIMPL.into());
        }
        let items = (0..ITEMS.len()).map(|i| Command::new(Some(i)).into()).collect();
        Ok(Commands { items, pos: Cell::new(0) }.into())
    }
}

impl Command {
    fn state(&self, items: Ref<IShellItemArray>) -> Result<u32> {
        if !enabled() {
            return Ok(ECS_HIDDEN.0 as u32);
        }
        let Some(i) = self.item else { return Ok(ECS_ENABLED.0 as u32) };
        if ITEMS[i].2 {
            // Extracting: only when every selected item is an archive
            let extensions = archive_extensions();
            let paths = paths(items);
            if paths.is_empty() || !paths.iter().all(|p| is_archive(p, &extensions)) {
                return Ok(ECS_HIDDEN.0 as u32);
            }
        }
        Ok(ECS_ENABLED.0 as u32)
    }

    fn invoke(&self, items: Ref<IShellItemArray>) -> Result<()> {
        let Some(i) = self.item else { return Ok(()) };
        let exe = read_string("Exe").ok_or(E_FAIL)?;
        let paths = paths(items);
        if paths.is_empty() {
            return Ok(());
        }
        // All the paths at once, in a file: a command line has a length limit
        let list = std::env::temp_dir().join(format!(
            "arc-menu-{}-{}.txt",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos())
        ));
        let written = std::fs::File::create(&list).and_then(|mut f| f.write_all(paths.join("\n").as_bytes()));
        if written.is_err() {
            return Err(E_FAIL.into());
        }
        // Raw arguments: the paths are quoted exactly the way Arc parses them
        let args = format!("{} --list \"{}\"", ITEMS[i].1, list.display());
        std::process::Command::new(&exe)
            .raw_arg(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map(drop)
            .map_err(|_| E_FAIL.into())
    }
}

#[implement(IEnumExplorerCommand, Agile = false)]
struct Commands {
    items: Vec<IExplorerCommand>,
    pos: Cell<usize>,
}

impl IEnumExplorerCommand_Impl for Commands_Impl {
    fn Next(&self, count: u32, out: *mut Option<IExplorerCommand>, fetched: *mut u32) -> HRESULT {
        if out.is_null() {
            return E_FAIL;
        }
        let mut n = 0u32;
        while n < count && self.pos.get() < self.items.len() {
            unsafe { out.add(n as usize).write(Some(self.items[self.pos.get()].clone())) };
            self.pos.set(self.pos.get() + 1);
            n += 1;
        }
        if !fetched.is_null() {
            unsafe { *fetched = n };
        }
        if n == count { S_OK } else { S_FALSE }
    }

    fn Skip(&self, count: u32) -> Result<()> {
        self.pos.set((self.pos.get() + count as usize).min(self.items.len()));
        Ok(())
    }

    fn Reset(&self) -> Result<()> {
        self.pos.set(0);
        Ok(())
    }

    fn Clone(&self) -> Result<IEnumExplorerCommand> {
        Ok(Commands { items: self.items.clone(), pos: Cell::new(self.pos.get()) }.into())
    }
}

#[implement(IClassFactory, Agile = false)]
struct Factory;

impl IClassFactory_Impl for Factory_Impl {
    fn CreateInstance(&self, outer: Ref<IUnknown>, iid: *const GUID, object: *mut *mut c_void) -> Result<()> {
        if !object.is_null() {
            unsafe { *object = std::ptr::null_mut() };
        }
        if outer.is_some() {
            return Err(CLASS_E_NOAGGREGATION.into());
        }
        let command: IExplorerCommand = Command::new(None).into();
        unsafe { command.query(iid, object).ok() }
    }

    fn LockServer(&self, _lock: BOOL) -> Result<()> {
        Ok(())
    }
}

#[unsafe(no_mangle)]
extern "system" fn DllGetClassObject(clsid: *const GUID, iid: *const GUID, object: *mut *mut c_void) -> HRESULT {
    if object.is_null() || clsid.is_null() {
        return E_FAIL;
    }
    unsafe { *object = std::ptr::null_mut() };
    if unsafe { *clsid } != CLSID {
        return CLASS_E_CLASSNOTAVAILABLE;
    }
    let factory: IClassFactory = Factory.into();
    unsafe { factory.query(iid, object) }
}

/// Stays loaded: Explorer keeps asking for the menu anyway, and nothing has to be counted.
#[unsafe(no_mangle)]
extern "system" fn DllCanUnloadNow() -> HRESULT {
    S_FALSE
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The way Explorer gets the menu: class object → instance → the "Arc" item → its submenu.
    #[test]
    fn com_object() {
        unsafe {
            let mut factory: *mut c_void = std::ptr::null_mut();
            assert_eq!(DllGetClassObject(&CLSID, &IClassFactory::IID, &mut factory), S_OK);
            let factory = IClassFactory::from_raw(factory);
            let root: IExplorerCommand = factory.CreateInstance(None).unwrap();
            assert_eq!(root.GetFlags().unwrap(), ECF_HASSUBCOMMANDS.0 as u32);
            let title = root.GetTitle(None).unwrap();
            assert_eq!(title.to_string().unwrap(), "Arc");
            CoTaskMemFree(Some(title.0 as *const c_void));

            let list = root.EnumSubCommands().unwrap();
            let mut got = [None, None, None, None, None, None];
            let mut fetched = 0;
            assert_eq!(list.Next(&mut got, Some(&mut fetched)), S_FALSE);
            assert_eq!(fetched, 5);
            assert!(got[4].is_some() && got[5].is_none());

            let mut wrong: *mut c_void = std::ptr::null_mut();
            assert_eq!(DllGetClassObject(&GUID::zeroed(), &IClassFactory::IID, &mut wrong), CLASS_E_CLASSNOTAVAILABLE);
        }
    }

    #[test]
    fn archive_names() {
        let ext: Vec<String> = ["zip", "gz", "7z"].iter().map(|s| s.to_string()).collect();
        assert!(super::is_archive(r"C:\a\b.ZIP", &ext));
        assert!(super::is_archive(r"C:\a\b.tar.gz", &ext));
        assert!(!super::is_archive(r"C:\a\b.zip.txt", &ext));
        assert!(!super::is_archive(r"C:\a\.zip", &ext));
    }

    #[test]
    fn folder_names() {
        let one = |p: &str| super::folder_name(&[p.to_string()]);
        assert_eq!(one(r"C:\a\photos.tar.gz"), "photos");
        assert_eq!(one(r"C:\a\Movie.Part01.RAR"), "Movie");
        assert_eq!(one(r"C:\a\v1.2.zip"), "v1.2");
        assert_eq!(super::folder_name(&[r"C:\a.zip".into(), r"C:\b.zip".into()]), "*");
    }
}
