//! Windows integration: the Explorer context menu (like 7-Zip's), "Open with", the system accent color.

use std::collections::HashMap;

/// Translations from lang/*.json for the texts Windows shows.
pub struct Strings {
    map: HashMap<String, String>,
    fallback: HashMap<String, String>,
}

fn parse(json: &str) -> HashMap<String, String> {
    serde_json::from_str(json).unwrap_or_default()
}

impl Strings {
    pub fn new(lang: &str) -> Strings {
        let fallback = parse(include_str!("../../../lang/en.json"));
        let map = match lang {
            "uk" => parse(include_str!("../../../lang/uk.json")),
            _ => fallback.clone(),
        };
        Strings { map, fallback }
    }

    pub fn t(&self, key: &str) -> String {
        self.map.get(key).or_else(|| self.fallback.get(key)).cloned().unwrap_or_else(|| key.to_string())
    }
}

#[cfg(windows)]
mod windows {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    use super::Strings;
    use crate::archive::ARCHIVE_EXTENSIONS;

    const CLASSES: &str = r"Software\Classes";
    const PROG_ID: &str = "Arc.Archive";

    type Res = std::io::Result<()>;

    /// Command line flag and translation key of each submenu item.
    const PACK: [(&str, &str, &str); 2] = [("01add", "--add", "menu_add"), ("02password", "--add-password", "menu_add_password")];
    const UNPACK: [(&str, &str, &str); 2] =
        [("03extract", "--extract-here", "menu_extract_here"), ("04folder", "--extract-folder", "menu_extract_folder")];

    /// A cascading "Arc" verb with the given items. Explorer starts one process per selected
    /// file; the running instance collects them into one batch (lib.rs).
    fn add_menu(classes: &RegKey, key: &str, items: &[(&str, &str, &str)], exe: &str, strings: &Strings) -> Res {
        let _ = classes.delete_subkey_all(key);
        let (menu, _) = classes.create_subkey(key)?;
        menu.set_value("MUIVerb", &"Arc")?;
        menu.set_value("Icon", &format!("\"{exe}\",0"))?;
        menu.set_value("SubCommands", &"")?;
        // Without it Explorer hides the verb when more than 15 files are selected
        menu.set_value("MultiSelectModel", &"Player")?;
        for (name, flag, text) in items {
            let (item, _) = menu.create_subkey(format!(r"shell\{name}"))?;
            item.set_value("MUIVerb", &strings.t(text))?;
            item.set_value("Icon", &format!("\"{exe}\",0"))?;
            item.create_subkey("command")?.0.set_value("", &format!("\"{exe}\" {flag} \"%1\""))?;
        }
        Ok(())
    }

    /// The Explorer context menu (Settings > Explorer context menu).
    pub fn register(language: &str) -> Res {
        let strings = Strings::new(language);
        let exe = std::env::current_exe()?.to_string_lossy().into_owned();
        let classes = RegKey::predef(HKEY_CURRENT_USER).create_subkey(CLASSES)?.0;
        let all: Vec<_> = PACK.iter().chain(UNPACK.iter()).copied().collect();

        // Any file and folder: pack. Archives get the full menu under the same verb name,
        // which Explorer prefers over the generic one, so it appears once.
        add_menu(&classes, r"*\shell\Arc", &PACK, &exe, &strings)?;
        add_menu(&classes, r"Directory\shell\Arc", &PACK, &exe, &strings)?;
        for ext in ARCHIVE_EXTENSIONS {
            add_menu(&classes, &format!(r"SystemFileAssociations\.{ext}\shell\Arc"), &all, &exe, &strings)?;
        }
        Ok(())
    }

    pub fn unregister() -> Res {
        let classes = RegKey::predef(HKEY_CURRENT_USER).create_subkey(CLASSES)?.0;
        let _ = classes.delete_subkey_all(r"*\shell\Arc");
        let _ = classes.delete_subkey_all(r"Directory\shell\Arc");
        for ext in ARCHIVE_EXTENSIONS {
            let _ = classes.delete_subkey_all(format!(r"SystemFileAssociations\.{ext}\shell\Arc"));
        }
        Ok(())
    }

    const CAPABILITIES: &str = r"Software\Arc\Capabilities";

    /// Arc as a program that can open archives: "Open with", and its page in
    /// Settings > Apps > Default apps. Done on every start; it changes no default.
    pub fn register_app(language: &str) -> Res {
        let strings = Strings::new(language);
        let exe = std::env::current_exe()?.to_string_lossy().into_owned();
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let classes = hkcu.create_subkey(CLASSES)?.0;

        let (prog, _) = classes.create_subkey(PROG_ID)?;
        prog.set_value("", &strings.t("archive_type"))?;
        prog.create_subkey("DefaultIcon")?.0.set_value("", &format!("\"{exe}\",0"))?;
        prog.create_subkey(r"shell\open\command")?.0.set_value("", &format!("\"{exe}\" \"%1\""))?;
        for ext in ARCHIVE_EXTENSIONS {
            classes.create_subkey(format!(r".{ext}\OpenWithProgids"))?.0.set_value(PROG_ID, &"")?;
        }

        let (caps, _) = hkcu.create_subkey(CAPABILITIES)?;
        caps.set_value("ApplicationName", &"Arc")?;
        caps.set_value("ApplicationDescription", &strings.t("about_desc"))?;
        let (assoc, _) = caps.create_subkey("FileAssociations")?;
        for ext in ARCHIVE_EXTENSIONS {
            assoc.set_value(format!(".{ext}"), &PROG_ID)?;
        }
        hkcu.create_subkey(r"Software\RegisteredApplications")?.0.set_value("Arc", &CAPABILITIES)?;
        Ok(())
    }

    /// Makes Arc the default for every archive type as far as a program may: the class default of
    /// each extension points to Arc. The rest is the user's click in Settings (the caller opens it).
    pub fn associate(language: &str) -> Res {
        register_app(language)?;
        let classes = RegKey::predef(HKEY_CURRENT_USER).create_subkey(CLASSES)?.0;
        for ext in ARCHIVE_EXTENSIONS {
            // Programs that read the class default directly (and older Windows) follow it.
            // Explorer on Windows 10/11 only follows the user's choice (UserChoice / UserChoiceLatest),
            // which only Windows itself may write: removing another program's choice doesn't make
            // Arc the default, it only leaves the type without one. So choices are never touched.
            classes.create_subkey(format!(".{ext}"))?.0.set_value("", &PROG_ID)?;
        }
        notify_associations();
        Ok(())
    }

    /// Hands the archive types back: the class defaults that point to Arc are removed.
    pub fn unassociate() -> Res {
        let classes = RegKey::predef(HKEY_CURRENT_USER).create_subkey(CLASSES)?.0;
        for ext in ARCHIVE_EXTENSIONS {
            if let Ok(key) = classes.open_subkey_with_flags(format!(".{ext}"), winreg::enums::KEY_ALL_ACCESS)
                && key.get_value::<String, _>("").is_ok_and(|p| p == PROG_ID)
            {
                let _ = key.delete_value("");
            }
        }
        notify_associations();
        Ok(())
    }

    fn notify_associations() {
        use windows_sys::Win32::UI::Shell::{SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify};
        unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED as i32, SHCNF_IDLIST, std::ptr::null(), std::ptr::null()) };
    }

    /// What Windows really opens each archive type with (the user's choice included).
    pub fn associations() -> Vec<super::Association> {
        use windows_sys::Win32::UI::Shell::{ASSOCF_INIT_IGNOREUNKNOWN, ASSOCSTR_FRIENDLYAPPNAME, ASSOCSTR_PROGID, AssocQueryStringW};
        let query = |ext: &str, what| -> Option<String> {
            let assoc: Vec<u16> = format!(".{ext}").encode_utf16().chain([0]).collect();
            let mut buf = vec![0u16; 512];
            let mut len = buf.len() as u32;
            let hr = unsafe {
                AssocQueryStringW(ASSOCF_INIT_IGNOREUNKNOWN, what, assoc.as_ptr(), std::ptr::null(), buf.as_mut_ptr(), &mut len)
            };
            (hr == 0).then(|| String::from_utf16_lossy(&buf[..len.saturating_sub(1) as usize]))
        };
        ARCHIVE_EXTENSIONS
            .iter()
            .map(|ext| {
                let prog_id = query(ext, ASSOCSTR_PROGID);
                super::Association {
                    ext: ext.to_string(),
                    is_arc: prog_id.as_deref() == Some(PROG_ID),
                    app: query(ext, ASSOCSTR_FRIENDLYAPPNAME).filter(|s| !s.is_empty()),
                }
            })
            .collect()
    }

    /// Arc's page in Settings > Apps > Default apps: "Set default" there takes every type at once.
    pub fn open_default_apps() {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "", "ms-settings:defaultapps?registeredAppUser=Arc"])
            .spawn();
    }

    /// The accent color of Windows (Settings > Personalization > Colors) as #rrggbb.
    pub fn system_accent() -> Option<String> {
        let dwm = RegKey::predef(HKEY_CURRENT_USER).open_subkey(r"Software\Microsoft\Windows\DWM").ok()?;
        // AccentColor is 0xAABBGGRR
        let abgr: u32 = dwm.get_value("AccentColor").ok()?;
        Some(format!("#{:02x}{:02x}{:02x}", abgr & 0xFF, (abgr >> 8) & 0xFF, (abgr >> 16) & 0xFF))
    }
}

/// Which program opens one archive type.
#[derive(serde::Serialize)]
pub struct Association {
    pub ext: String,
    pub is_arc: bool,
    /// Name of the program that opens it now
    pub app: Option<String>,
}

#[cfg(windows)]
pub use windows::{associate, associations, open_default_apps, register, register_app, system_accent, unassociate, unregister};

#[cfg(not(windows))]
pub fn register(_language: &str) -> std::io::Result<()> {
    Ok(())
}

#[cfg(not(windows))]
pub fn unregister() -> std::io::Result<()> {
    Ok(())
}

#[cfg(not(windows))]
pub fn register_app(_language: &str) -> std::io::Result<()> {
    Ok(())
}

#[cfg(not(windows))]
pub fn associate(_language: &str) -> std::io::Result<()> {
    Ok(())
}

#[cfg(not(windows))]
pub fn unassociate() -> std::io::Result<()> {
    Ok(())
}

#[cfg(not(windows))]
pub fn associations() -> Vec<Association> {
    Vec::new()
}

#[cfg(not(windows))]
pub fn open_default_apps() {}

#[cfg(not(windows))]
pub fn system_accent() -> Option<String> {
    None
}

#[cfg(all(test, windows))]
mod tests {
    /// Read-only: every archive type gets an answer, and the names come out readable.
    #[test]
    fn associations_are_queried() {
        let list = super::associations();
        assert_eq!(list.len(), crate::archive::ARCHIVE_EXTENSIONS.len());
        for a in &list {
            println!(".{:<4} arc={} app={:?}", a.ext, a.is_arc, a.app);
        }
    }
}
