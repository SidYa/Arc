//! ZIP: AES-256 for new encrypted archives; ZipCrypto and AES when reading.

use std::fs::{self, File};
use std::io::{BufReader, Seek, Write};
use std::path::Path;

use zip::result::ZipError;
use zip::write::{FileOptions, SimpleFileOptions};
use zip::{AesMode, CompressionMethod, DateTime, ZipArchive, ZipWriter};

use super::{Entry, Error, ExtractOptions, Format, Listing, Progress, Result, Source};

/// General purpose flag: the name is UTF-8
const FLAG_UTF8: u16 = 0x0800;

fn open(path: &Path) -> Result<ZipArchive<BufReader<File>>> {
    ZipArchive::new(BufReader::new(File::open(path)?)).map_err(map_error)
}

fn map_error(e: ZipError) -> Error {
    match e {
        ZipError::InvalidPassword => Error::WrongPassword,
        ZipError::Io(io) => io.into(),
        other => Error::Other(other.to_string()),
    }
}

/// Names without the UTF-8 flag that aren't valid UTF-8 come from the OEM code page:
/// Explorer and old archivers write Cyrillic names in CP866 (Russian) or CP1125 (Ukrainian).
fn decode_name(raw: &[u8], flags: u16) -> String {
    if flags & FLAG_UTF8 != 0 || raw.is_ascii() {
        return String::from_utf8_lossy(raw).into_owned();
    }
    match std::str::from_utf8(raw) {
        Ok(s) => s.to_string(),
        Err(_) => decode_oem(raw, guess_code_page(raw)),
    }
}

/// The archive may come from another machine: an English Windows (OEM 437) still gets
/// Cyrillic archives. Cyrillic words are runs of high bytes in the letter ranges of CP866,
/// while Western names have single accented letters between ASCII ones.
fn guess_code_page(raw: &[u8]) -> u32 {
    let system = oem_code_page();
    if matches!(system, 866 | 1125) {
        return system;
    }
    let letter = |b: u8| matches!(b, 0x80..=0xAF | 0xE0..=0xF9);
    let longest_run = raw.split(|&b| !letter(b)).map(|run| run.len()).max().unwrap_or(0);
    if longest_run >= 3 || cyrillic_locale() {
        // Ў ў ° ∙ of CP866 never appear in names: there they are І і Ї ї of CP1125
        if raw.iter().any(|b| (0xF6..=0xF9).contains(b)) { 1125 } else { 866 }
    } else {
        system
    }
}

fn cyrillic_locale() -> bool {
    let locale = sys_locale::get_locale().unwrap_or_default().to_lowercase();
    ["uk", "ru", "be", "bg", "sr", "mk", "kk", "ky", "tg", "mn"].iter().any(|l| locale.starts_with(l))
}

pub(super) fn decode_oem(raw: &[u8], code_page: u32) -> String {
    match code_page {
        // CP1125 is CP866 with the Ukrainian letters in 0xF2..=0xF9
        1125 => raw
            .iter()
            .map(|&b| match b {
                0xF2 => 'Ґ',
                0xF3 => 'ґ',
                0xF4 => 'Є',
                0xF5 => 'є',
                0xF6 => 'І',
                0xF7 => 'і',
                0xF8 => 'Ї',
                0xF9 => 'ї',
                _ => encoding_rs::IBM866.decode_without_bom_handling(&[b]).0.chars().next().unwrap_or('_'),
            })
            .collect(),
        866 => encoding_rs::IBM866.decode_without_bom_handling(raw).0.into_owned(),
        // Western code pages: CP437, as in the ZIP specification
        _ => raw.iter().map(|&b| if b < 0x80 { b as char } else { CP437_HIGH[(b - 0x80) as usize] }).collect(),
    }
}

const CP437_HIGH: [char; 128] = [
    'Ç', 'ü', 'é', 'â', 'ä', 'à', 'å', 'ç', 'ê', 'ë', 'è', 'ï', 'î', 'ì', 'Ä', 'Å', 'É', 'æ', 'Æ', 'ô', 'ö', 'ò', 'û', 'ù',
    'ÿ', 'Ö', 'Ü', '¢', '£', '¥', '₧', 'ƒ', 'á', 'í', 'ó', 'ú', 'ñ', 'Ñ', 'ª', 'º', '¿', '⌐', '¬', '½', '¼', '¡', '«', '»',
    '░', '▒', '▓', '│', '┤', '╡', '╢', '╖', '╕', '╣', '║', '╗', '╝', '╜', '╛', '┐', '└', '┴', '┬', '├', '─', '┼', '╞', '╟',
    '╚', '╔', '╩', '╦', '╠', '═', '╬', '╧', '╨', '╤', '╥', '╙', '╘', '╒', '╓', '╫', '╪', '┘', '┌', '█', '▄', '▌', '▐', '▀',
    'α', 'ß', 'Γ', 'π', 'Σ', 'σ', 'µ', 'τ', 'Φ', 'Θ', 'Ω', 'δ', '∞', 'φ', 'ε', '∩', '≡', '±', '≥', '≤', '⌠', '⌡', '÷', '≈',
    '°', '∙', '·', '√', 'ⁿ', '²', '■', '\u{A0}',
];

/// The system OEM code page; Cyrillic CP866 when unknown, the most common source of such archives.
fn oem_code_page() -> u32 {
    static CODE_PAGE: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *CODE_PAGE.get_or_init(|| {
        #[cfg(windows)]
        {
            use winreg::RegKey;
            use winreg::enums::HKEY_LOCAL_MACHINE;
            let key = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(r"SYSTEM\CurrentControlSet\Control\Nls\CodePage");
            if let Some(cp) = key.ok().and_then(|k| k.get_value::<String, _>("OEMCP").ok()).and_then(|v| v.parse().ok()) {
                return cp;
            }
        }
        866
    })
}

fn unix_time(dt: Option<DateTime>) -> Option<i64> {
    let dt = dt?;
    super::dos_to_unix(dt.datepart(), dt.timepart())
}

pub fn list(path: &Path, _password: Option<&str>) -> Result<Listing> {
    let archive = open(path)?;
    let mut entries = Vec::with_capacity(archive.len());
    let mut encrypted = false;
    for i in 0..archive.len() {
        let file = archive.by_index_data(i).map_err(map_error)?;
        let name = super::normalize(&decode_name(file.name_raw(), file.flags().as_u16()));
        if name.is_empty() {
            continue;
        }
        encrypted |= file.encrypted();
        entries.push(Entry {
            path: name,
            is_dir: file.is_dir(),
            size: file.size(),
            packed: file.compressed_size(),
            modified: unix_time(file.last_modified()),
            encrypted: file.encrypted(),
        });
    }
    Ok(Listing { format: Format::Zip, entries, encrypted, solid: false, size: 0 })
}

pub fn extract(path: &Path, opts: &ExtractOptions, progress: &Progress) -> Result<()> {
    let mut archive = open(path)?;

    // What to extract, decided up front for the total size
    let mut work = Vec::new();
    for i in 0..archive.len() {
        let file = archive.by_index_data(i).map_err(map_error)?;
        let name = decode_name(file.name_raw(), file.flags().as_u16());
        if let Some(relative) = opts.target(&name) {
            if file.encrypted() && opts.password.is_none() {
                return Err(Error::PasswordRequired);
            }
            work.push((i, relative, file.is_dir(), file.size(), unix_time(file.last_modified())));
        }
    }
    progress.set_total(work.iter().map(|w| w.3).sum());

    for (index, relative, is_dir, _, modified) in work {
        progress.check()?;
        progress.set_current(&relative);
        if is_dir {
            if let Some(dir) = opts.dir_path(&relative) {
                fs::create_dir_all(dir)?;
            }
            continue;
        }
        let mut entry = match opts.password {
            Some(password) if archive.by_index_data(index).map_err(map_error)?.encrypted() => {
                archive.by_index_decrypt(index, password.as_bytes())
            }
            _ => archive.by_index(index),
        }
        .map_err(map_error)?;
        let Some((target, file)) = super::create_file(opts, &relative)? else { continue };
        let mut out = std::io::BufWriter::with_capacity(1 << 20, file);
        let result = super::copy(&mut entry, &mut out, progress).and_then(|_| out.flush());
        drop(out);
        if let Err(e) = result {
            let _ = fs::remove_file(&target);
            // A ZipCrypto password is checked by one byte only: a wrong one shows up as bad data
            if opts.password.is_some() && e.kind() == std::io::ErrorKind::InvalidData {
                return Err(Error::WrongPassword);
            }
            return Err(e.into());
        }
        super::set_modified(&target, modified);
    }
    Ok(())
}

fn zip_time(source: &Source) -> DateTime {
    use chrono::{Datelike, Local, Timelike};
    let local: chrono::DateTime<Local> = source.modified.into();
    DateTime::from_date_and_time(
        local.year().clamp(1980, 2107) as u16,
        local.month() as u8,
        local.day() as u8,
        local.hour() as u8,
        local.minute() as u8,
        local.second() as u8,
    )
    .unwrap_or_default()
}

pub fn create<W: Write + Seek>(
    out: W,
    sources: &[Source],
    level: u32,
    password: Option<&str>,
    progress: &Progress,
) -> Result<()> {
    let mut zip = ZipWriter::new(out);
    let base: SimpleFileOptions = if level == 0 {
        SimpleFileOptions::default().compression_method(CompressionMethod::Stored)
    } else {
        SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .compression_level(Some(level.min(9) as i64))
    };
    for source in sources {
        progress.check()?;
        progress.set_current(&source.name);
        let options = base.last_modified_time(zip_time(source)).large_file(source.size >= u32::MAX as u64);
        if source.is_dir {
            zip.add_directory(format!("{}/", source.name), options).map_err(map_error)?;
            continue;
        }
        let options: FileOptions<'_, '_, ()> = match password {
            Some(p) => options.with_aes_encryption(AesMode::Aes256, p),
            None => options,
        };
        zip.start_file(&source.name, options).map_err(map_error)?;
        let mut file = File::open(&source.path)?;
        super::copy(&mut file, &mut zip, progress)?;
    }
    let mut out = zip.finish().map_err(map_error)?;
    out.flush()?;
    Ok(())
}
