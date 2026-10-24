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

        // "Open with" Arc, without taking over the default program
        let (prog, _) = classes.create_subkey(PROG_ID)?;
        prog.set_value("", &strings.t("archive_type"))?;
        prog.create_subkey("DefaultIcon")?.0.set_value("", &format!("\"{exe}\",0"))?;
        prog.create_subkey(r"shell\open\command")?.0.set_value("", &format!("\"{exe}\" \"%1\""))?;
        for ext in ARCHIVE_EXTENSIONS {
            classes.create_subkey(format!(r".{ext}\OpenWithProgids"))?.0.set_value(PROG_ID, &"")?;
        }
        Ok(())
    }

    pub fn unregister() -> Res {
        let classes = RegKey::predef(HKEY_CURRENT_USER).create_subkey(CLASSES)?.0;
        let _ = classes.delete_subkey_all(r"*\shell\Arc");
        let _ = classes.delete_subkey_all(r"Directory\shell\Arc");
        let _ = classes.delete_subkey_all(PROG_ID);
        for ext in ARCHIVE_EXTENSIONS {
            let _ = classes.delete_subkey_all(format!(r"SystemFileAssociations\.{ext}\shell\Arc"));
            if let Ok(key) = classes.open_subkey_with_flags(format!(r".{ext}\OpenWithProgids"), winreg::enums::KEY_ALL_ACCESS) {
                let _ = key.delete_value(PROG_ID);
            }
        }
        Ok(())
    }

    /// The accent color of Windows (Settings > Personalization > Colors) as #rrggbb.
    pub fn system_accent() -> Option<String> {
        let dwm = RegKey::predef(HKEY_CURRENT_USER).open_subkey(r"Software\Microsoft\Windows\DWM").ok()?;
        // AccentColor is 0xAABBGGRR
        let abgr: u32 = dwm.get_value("AccentColor").ok()?;
        Some(format!("#{:02x}{:02x}{:02x}", abgr & 0xFF, (abgr >> 8) & 0xFF, (abgr >> 16) & 0xFF))
    }
}

#[cfg(windows)]
pub use windows::{register, system_accent, unregister};

#[cfg(not(windows))]
pub fn register(_language: &str) -> std::io::Result<()> {
    Ok(())
}

#[cfg(not(windows))]
pub fn unregister() -> std::io::Result<()> {
    Ok(())
}

#[cfg(not(windows))]
pub fn system_accent() -> Option<String> {
    None
}
