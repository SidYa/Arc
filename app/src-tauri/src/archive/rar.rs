//! RAR (read only, through the official UnRAR library). Multi-volume archives start from the first part.

use std::fs;
use std::path::Path;

use unrar::Archive;
use unrar::error::{Code, UnrarError};

use super::{Entry, Error, ExtractOptions, Format, Listing, Progress, Result};

fn map_error(e: UnrarError, has_password: bool) -> Error {
    match e.code {
        Code::MissingPassword => Error::PasswordRequired,
        Code::BadPassword => Error::WrongPassword,
        // A wrong key for encrypted data fails the checksum
        Code::BadData if has_password => Error::WrongPassword,
        _ => Error::Other(format!("RAR: {e}")),
    }
}

/// The unrar crate rewrites any `name.<digits>.rar` into a guessed first volume of a multipart set
/// (`movie.2024.rar` -> `movie.0001.rar`, which does not exist). Keep that substitution only when
/// part 1 is really on disk, otherwise open the archive as it is.
fn with_archive<R>(path: &Path, password: Option<&str>, f: impl for<'a> FnOnce(Archive<'a>) -> R) -> R {
    let part = Archive::new(path).first_part_option().filter(|p| p.as_path() != path && p.exists());
    let file: &Path = part.as_deref().unwrap_or(path);
    let archive = match password {
        Some(p) => Archive::with_password(file, p),
        None => Archive::new(file),
    };
    f(archive)
}

fn unix_time(dos: u32) -> Option<i64> {
    super::dos_to_unix((dos >> 16) as u16, dos as u16)
}

pub fn list(path: &Path, password: Option<&str>) -> Result<Listing> {
    let has_password = password.is_some();
    let opened = with_archive(path, password, |a| {
        a.open_for_listing().map_err(|e| map_error(e, has_password))
    })?;
    let solid = opened.is_solid();
    let mut entries: Vec<Entry> = Vec::new();
    let mut encrypted = opened.has_encrypted_headers();
    for header in opened {
        let header = header.map_err(|e| map_error(e, has_password))?;
        let name = super::normalize(&header.filename.to_string_lossy());
        if name.is_empty() {
            continue;
        }
        encrypted |= header.is_encrypted();
        // A file split across volumes is listed once per volume
        if let Some(last) = entries.last_mut()
            && last.path == name
        {
            continue;
        }
        entries.push(Entry {
            path: name,
            is_dir: header.is_directory(),
            size: header.unpacked_size,
            packed: 0,
            modified: unix_time(header.file_time),
            encrypted: header.is_encrypted(),
        });
    }
    Ok(Listing { format: Format::Rar, entries, encrypted, solid, size: 0 })
}

pub fn extract(path: &Path, opts: &ExtractOptions, progress: &Progress) -> Result<()> {
    let has_password = opts.password.is_some();
    // The total comes from a listing pass (headers only, fast)
    let listing = list(path, opts.password)?;
    let wanted: Vec<&Entry> = listing.entries.iter().filter(|e| !e.is_dir && opts.target(&e.path).is_some()).collect();
    progress.set_total(wanted.iter().map(|e| e.size).sum());
    // A solid archive is unpacked from the start anyway, but never past the last wanted file
    let mut left = wanted.len();
    if left == 0 && opts.items.is_some() && listing.entries.iter().all(|e| !e.is_dir || opts.target(&e.path).is_none()) {
        return Ok(());
    }

    let mut cursor = with_archive(path, opts.password.as_deref(), |a| {
        a.open_for_processing().map_err(|e| map_error(e, has_password))
    })?;
    let mut last_name = String::new();
    loop {
        progress.check()?;
        let Some(header) = cursor.read_header().map_err(|e| map_error(e, has_password))? else { break };
        let entry = header.entry();
        let name = entry.filename.to_string_lossy().into_owned();
        let target = opts.target(&name);
        let continued = super::normalize(&name) == last_name;
        last_name = super::normalize(&name);

        cursor = match target {
            Some(relative) if entry.is_directory() => {
                if let Some(dir) = opts.dir_path(&relative) {
                    fs::create_dir_all(dir)?;
                }
                header.skip()
            }
            Some(relative) if !continued => {
                progress.set_current(&relative);
                let size = entry.unpacked_size;
                let modified = unix_time(entry.file_time);
                match opts.file_path(&relative) {
                    Some(target) => {
                        if let Some(parent) = target.parent() {
                            fs::create_dir_all(parent)?;
                        }
                        // UnRAR writes the file itself: its growing size is the progress
                        progress.watch(Some(target.clone()));
                        let result = header.extract_to(&target);
                        progress.watch(None);
                        match result {
                            Ok(next) => {
                                progress.add(size);
                                super::set_modified(&target, modified);
                                left = left.saturating_sub(1);
                                if left == 0 && opts.items.is_some() {
                                    break;
                                }
                                Ok(next)
                            }
                            Err(e) => {
                                let _ = fs::remove_file(&target);
                                return Err(map_error(e, has_password));
                            }
                        }
                    }
                    None => {
                        left = left.saturating_sub(1);
                        header.skip()
                    }
                }
            }
            _ => header.skip(),
        }
        .map_err(|e| map_error(e, has_password))?;
    }
    Ok(())
}
