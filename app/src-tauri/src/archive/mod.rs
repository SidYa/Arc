//! Archive formats behind one interface: list, extract (all or some entries) and create.

pub mod rar;
pub mod sevenz;
pub mod tar;
pub mod zip;

use std::collections::HashSet;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format {
    #[serde(rename = "zip")]
    Zip,
    #[serde(rename = "7z")]
    SevenZ,
    #[serde(rename = "tar.gz")]
    TarGz,
    #[serde(rename = "tar.xz")]
    TarXz,
    #[serde(rename = "tar")]
    Tar,
    /// A single gzip-compressed file (not a tarball)
    #[serde(rename = "gz")]
    Gz,
    /// A single xz-compressed file (not a tarball)
    #[serde(rename = "xz")]
    Xz,
    #[serde(rename = "rar")]
    Rar,
}

/// Extensions Explorer gets the extract commands for.
pub const ARCHIVE_EXTENSIONS: &[&str] = &["zip", "7z", "rar", "tar", "gz", "tgz", "xz", "txz"];

impl Format {
    /// By the file name, then by the signature for files with an unusual extension.
    pub fn detect(path: &Path) -> Option<Format> {
        let name = path.file_name()?.to_string_lossy().to_lowercase();
        let by_name = if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
            Some(Format::TarGz)
        } else if name.ends_with(".tar.xz") || name.ends_with(".txz") {
            Some(Format::TarXz)
        } else if name.ends_with(".tar") {
            Some(Format::Tar)
        } else if name.ends_with(".gz") {
            Some(Format::Gz)
        } else if name.ends_with(".xz") {
            Some(Format::Xz)
        } else if name.ends_with(".zip") {
            Some(Format::Zip)
        } else if name.ends_with(".7z") {
            Some(Format::SevenZ)
        } else if name.ends_with(".rar") {
            Some(Format::Rar)
        } else {
            None
        };
        by_name.or_else(|| Self::sniff(path))
    }

    fn sniff(path: &Path) -> Option<Format> {
        let mut head = [0u8; 8];
        let n = File::open(path).and_then(|mut f| f.read(&mut head)).ok()?;
        let head = &head[..n];
        if head.starts_with(b"PK\x03\x04") || head.starts_with(b"PK\x05\x06") {
            Some(Format::Zip)
        } else if head.starts_with(b"7z\xBC\xAF\x27\x1C") {
            Some(Format::SevenZ)
        } else if head.starts_with(b"Rar!\x1A\x07") {
            Some(Format::Rar)
        } else if head.starts_with(b"\x1F\x8B") {
            Some(Format::TarGz)
        } else if head.starts_with(b"\xFD7zXZ\x00") {
            Some(Format::TarXz)
        } else {
            None
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Format::Zip => "zip",
            Format::SevenZ => "7z",
            Format::TarGz => "tar.gz",
            Format::TarXz => "tar.xz",
            Format::Tar => "tar",
            Format::Gz => "gz",
            Format::Xz => "xz",
            Format::Rar => "rar",
        }
    }
}

/// "photos.tar.gz" -> "photos", "movie.part1.rar" -> "movie": the folder name for "extract to folder".
pub fn archive_stem(path: &Path) -> String {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
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
        && stem[pos + 5..].chars().all(|c| c.is_ascii_digit())
        && stem.len() > pos + 5
    {
        stem.truncate(pos);
    }
    if stem.is_empty() { "archive".into() } else { stem }
}

// ------------------------------------------------------------------ errors

#[derive(Debug)]
pub enum Error {
    PasswordRequired,
    WrongPassword,
    Cancelled,
    Other(String),
}

impl fmt::Display for Error {
    /// The first three are codes the UI recognizes
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::PasswordRequired => f.write_str("password_required"),
            Error::WrongPassword => f.write_str("wrong_password"),
            Error::Cancelled => f.write_str("cancelled"),
            Error::Other(message) => f.write_str(message),
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        if e.kind() == io::ErrorKind::Interrupted && e.to_string() == "cancelled" {
            Error::Cancelled
        } else {
            Error::Other(e.to_string())
        }
    }
}

impl From<walkdir::Error> for Error {
    fn from(e: walkdir::Error) -> Self {
        Error::Other(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

// ------------------------------------------------------------------ listing

#[derive(Serialize, Clone, Debug)]
pub struct Entry {
    /// Relative path with "/" separators, no trailing slash
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    /// Compressed size, 0 when the format doesn't tell
    pub packed: u64,
    /// Unix seconds
    pub modified: Option<i64>,
    pub encrypted: bool,
}

#[derive(Serialize)]
pub struct Listing {
    pub format: Format,
    pub entries: Vec<Entry>,
    pub encrypted: bool,
    pub solid: bool,
    /// Size of the archive file itself
    pub size: u64,
}

pub fn list(path: &Path, password: Option<&str>, progress: &Progress) -> Result<Listing> {
    let format = Format::detect(path).ok_or_else(|| Error::Other("unknown_format".into()))?;
    let mut listing = match format {
        Format::Zip => zip::list(path, password)?,
        Format::SevenZ => sevenz::list(path, password)?,
        Format::Rar => rar::list(path, password)?,
        Format::TarGz | Format::TarXz | Format::Tar | Format::Gz | Format::Xz => tar::list(path, format, progress)?,
    };
    listing.size = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    Ok(listing)
}

/// Archive paths: "\" from Windows tools becomes "/", no leading "./" or "/".
pub fn normalize(name: &str) -> String {
    name.replace('\\', "/")
        .split('/')
        .filter(|p| !p.is_empty() && *p != ".")
        .collect::<Vec<_>>()
        .join("/")
}

// ------------------------------------------------------------------ progress

/// Shared between a running job and the thread reporting its progress.
#[derive(Default)]
pub struct Progress {
    pub total: AtomicU64,
    pub done: AtomicU64,
    pub current: Mutex<String>,
    cancel: AtomicBool,
    paused: AtomicBool,
    /// A file written by a C library (unrar): its growing size counts as done
    watch: Mutex<Option<PathBuf>>,
}

impl Progress {
    pub fn add(&self, n: u64) {
        self.done.fetch_add(n, Ordering::Relaxed);
    }

    pub fn set_total(&self, n: u64) {
        self.total.store(n, Ordering::Relaxed);
    }

    pub fn set_current(&self, name: &str) {
        *self.current.lock().unwrap() = name.to_string();
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::SeqCst);
    }

    /// Called between chunks: waits while paused, fails once cancelled.
    pub fn check(&self) -> io::Result<()> {
        while self.paused.load(Ordering::SeqCst) && !self.is_cancelled() {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        if self.is_cancelled() { Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled")) } else { Ok(()) }
    }

    pub fn watch(&self, path: Option<PathBuf>) {
        *self.watch.lock().unwrap() = path;
    }

    /// Done bytes, including the watched file
    pub fn snapshot(&self) -> (u64, u64) {
        let extra = self.watch.lock().unwrap().as_ref().and_then(|p| fs::metadata(p).ok()).map_or(0, |m| m.len());
        (self.done.load(Ordering::Relaxed) + extra, self.total.load(Ordering::Relaxed))
    }
}

/// Counts what passes through into the progress and stops on cancel.
pub struct Counted<'a, T> {
    pub inner: T,
    pub progress: &'a Progress,
}

impl<'a, T> Counted<'a, T> {
    pub fn new(inner: T, progress: &'a Progress) -> Self {
        Counted { inner, progress }
    }
}

impl<T: Read> Read for Counted<'_, T> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.progress.check()?;
        let n = self.inner.read(buf)?;
        self.progress.add(n as u64);
        Ok(n)
    }
}

impl<T: Write> Write for Counted<'_, T> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.progress.check()?;
        let n = self.inner.write(buf)?;
        self.progress.add(n as u64);
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Larger chunks than io::copy: fewer progress updates and syscalls.
pub fn copy(reader: &mut dyn Read, writer: &mut dyn Write, progress: &Progress) -> io::Result<u64> {
    let mut buf = vec![0u8; 256 * 1024];
    let mut total = 0;
    loop {
        progress.check()?;
        let n = match reader.read(&mut buf) {
            Ok(0) => return Ok(total),
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted && e.to_string() != "cancelled" => continue,
            Err(e) => return Err(e),
        };
        writer.write_all(&buf[..n])?;
        progress.add(n as u64);
        total += n as u64;
    }
}

// ------------------------------------------------------------------ extraction

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Default, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Conflict {
    /// "name (1).ext", the existing file stays
    #[default]
    Rename,
    Overwrite,
    Skip,
}

pub struct ExtractOptions<'a> {
    pub dest: &'a Path,
    pub password: Option<&'a str>,
    /// Only these entries (files, or folders with everything inside); all when None
    pub items: Option<HashSet<String>>,
    /// Folder inside the archive the items are taken from: it is not recreated
    pub base: String,
    pub conflict: Conflict,
}

impl ExtractOptions<'_> {
    /// Path relative to dest, or None when the entry is not wanted
    pub fn target(&self, entry: &str) -> Option<String> {
        let entry = normalize(entry);
        if let Some(items) = &self.items {
            let wanted = items.contains(&entry)
                || items.iter().any(|i| entry.len() > i.len() && entry.starts_with(i.as_str()) && entry.as_bytes()[i.len()] == b'/');
            if !wanted {
                return None;
            }
        }
        if self.base.is_empty() {
            return Some(entry);
        }
        let rest = entry.strip_prefix(self.base.as_str())?.strip_prefix('/')?;
        (!rest.is_empty()).then(|| rest.to_string())
    }

    /// Where an entry goes on disk; None for unsafe names or a skipped conflict
    pub fn file_path(&self, relative: &str) -> Option<PathBuf> {
        let path = safe_join(self.dest, relative)?;
        if !path.exists() {
            return Some(path);
        }
        match self.conflict {
            Conflict::Overwrite => Some(path),
            Conflict::Skip => None,
            Conflict::Rename => Some(unique_path(&path)),
        }
    }

    pub fn dir_path(&self, relative: &str) -> Option<PathBuf> {
        safe_join(self.dest, relative)
    }
}

/// Joins an untrusted archive path onto dest; never leaves dest (zip-slip).
pub fn safe_join(dest: &Path, name: &str) -> Option<PathBuf> {
    let mut path = dest.to_path_buf();
    let mut any = false;
    for part in name.replace('\\', "/").split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        let candidate = Path::new(part);
        if !matches!(candidate.components().next(), Some(Component::Normal(_))) || candidate.components().count() != 1 {
            return None;
        }
        path.push(sanitize_component(part));
        any = true;
    }
    any.then_some(path)
}

/// Characters Windows doesn't allow in names become "_"; trailing dots and spaces go.
fn sanitize_component(part: &str) -> String {
    if !cfg!(windows) {
        return part.to_string();
    }
    let cleaned: String = part
        .chars()
        .map(|c| if matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*') || (c as u32) < 32 { '_' } else { c })
        .collect();
    let trimmed = cleaned.trim_end_matches(['.', ' ']);
    if trimmed.is_empty() { "_".into() } else { trimmed.to_string() }
}

/// "file.txt" -> "file (1).txt" until the name is free.
pub fn unique_path(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let parent = path.parent().unwrap_or(Path::new(""));
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let (stem, ext) = if path.is_dir() {
        (name.as_str(), "")
    } else {
        match name.rfind('.') {
            Some(pos) if pos > 0 => (&name[..pos], &name[pos..]),
            _ => (name.as_str(), ""),
        }
    };
    (1..)
        .map(|i| parent.join(format!("{stem} ({i}){ext}")))
        .find(|p| !p.exists())
        .unwrap()
}

/// Creates the file (and its folder), returning None when the name is unsafe or skipped.
pub fn create_file(opts: &ExtractOptions, relative: &str) -> io::Result<Option<(PathBuf, File)>> {
    let Some(path) = opts.file_path(relative) else { return Ok(None) };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = File::create(&path)?;
    Ok(Some((path, file)))
}

pub fn set_modified(path: &Path, unix: Option<i64>) {
    if let Some(secs) = unix {
        let _ = filetime::set_file_mtime(path, filetime::FileTime::from_unix_time(secs, 0));
    }
}

pub fn extract(path: &Path, opts: &ExtractOptions, progress: &Progress) -> Result<()> {
    fs::create_dir_all(opts.dest)?;
    match Format::detect(path).ok_or_else(|| Error::Other("unknown_format".into()))? {
        Format::Zip => zip::extract(path, opts, progress),
        Format::SevenZ => sevenz::extract(path, opts, progress),
        Format::Rar => rar::extract(path, opts, progress),
        format => tar::extract(path, format, opts, progress),
    }
}

// ------------------------------------------------------------------ creation

#[derive(Deserialize, Clone, Debug)]
pub struct CreateOptions {
    pub format: Format,
    /// 0 store … 9 ultra
    pub level: u32,
    pub password: Option<String>,
    /// 7z: file names are hidden without the password too
    #[serde(default)]
    pub encrypt_names: bool,
}

/// A file or folder to put into the archive.
pub struct Source {
    pub path: PathBuf,
    /// Path inside the archive, "/" separators
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: SystemTime,
}

impl Source {
    pub fn unix_time(&self) -> i64 {
        self.modified.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
    }
}

/// Everything under the given paths; names are relative to each path's parent folder.
pub fn collect(paths: &[PathBuf], exclude: &[PathBuf]) -> Result<Vec<Source>> {
    let mut sources = Vec::new();
    let mut seen = HashSet::new();
    for root in paths {
        let parent = root.parent().unwrap_or(Path::new(""));
        for item in walkdir::WalkDir::new(root).follow_links(false).sort_by_file_name() {
            let item = item?;
            let file_type = item.file_type();
            if file_type.is_symlink() || exclude.iter().any(|e| e == item.path()) {
                continue;
            }
            let Ok(relative) = item.path().strip_prefix(parent) else { continue };
            let name = relative.to_string_lossy().replace('\\', "/");
            if name.is_empty() || !seen.insert(name.clone()) {
                continue;
            }
            let meta = item.metadata()?;
            sources.push(Source {
                path: item.path().to_path_buf(),
                name,
                is_dir: file_type.is_dir(),
                size: if file_type.is_dir() { 0 } else { meta.len() },
                modified: meta.modified().unwrap_or(SystemTime::now()),
            });
        }
    }
    Ok(sources)
}

/// Writes into "<dest>.part" and renames at the end, so a failed or cancelled job leaves nothing behind.
pub fn create(paths: &[PathBuf], dest: &Path, opts: &CreateOptions, progress: &Progress) -> Result<()> {
    let temp = dest.with_file_name(format!("{}.part", dest.file_name().unwrap_or_default().to_string_lossy()));
    let sources = collect(paths, &[dest.to_path_buf(), temp.clone()])?;
    if sources.is_empty() {
        return Err(Error::Other("nothing_to_archive".into()));
    }
    progress.set_total(sources.iter().map(|s| s.size).sum());
    let password = opts.password.as_deref().filter(|p| !p.is_empty());

    let result = (|| {
        let file = io::BufWriter::with_capacity(1 << 20, File::create(&temp)?);
        match opts.format {
            Format::Zip => zip::create(file, &sources, opts.level, password, progress),
            Format::SevenZ => sevenz::create(file, &sources, opts.level, password, opts.encrypt_names, progress),
            Format::TarGz | Format::TarXz | Format::Tar => tar::create(file, opts.format, &sources, opts.level, progress),
            _ => Err(Error::Other("unsupported_format".into())),
        }
    })();
    match result {
        Ok(()) => {
            if dest.exists() {
                fs::remove_file(dest)?;
            }
            fs::rename(&temp, dest)?;
            Ok(())
        }
        Err(e) => {
            let _ = fs::remove_file(&temp);
            Err(if progress.is_cancelled() { Error::Cancelled } else { e })
        }
    }
}

// ------------------------------------------------------------------ helpers

/// MS-DOS date and time (local time, used by ZIP and RAR) to Unix seconds.
pub fn dos_to_unix(date: u16, time: u16) -> Option<i64> {
    use chrono::{Local, NaiveDate, TimeZone};
    let day = NaiveDate::from_ymd_opt(1980 + (date >> 9) as i32, ((date >> 5) & 0xF) as u32, (date & 0x1F) as u32)?;
    let datetime = day.and_hms_opt((time >> 11) as u32, ((time >> 5) & 0x3F) as u32, ((time & 0x1F) * 2) as u32)?;
    Local.from_local_datetime(&datetime).earliest().map(|d| d.timestamp())
}

/// Reader that opens its file on the first read and closes it at the end,
/// so thousands of queued files don't hold thousands of handles.
pub struct LazyFile<'a> {
    path: PathBuf,
    name: String,
    file: Option<File>,
    finished: bool,
    progress: &'a Progress,
}

impl<'a> LazyFile<'a> {
    pub fn new(source: &Source, progress: &'a Progress) -> Self {
        LazyFile { path: source.path.clone(), name: source.name.clone(), file: None, finished: false, progress }
    }
}

impl Read for LazyFile<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.progress.check()?;
        if self.finished {
            return Ok(0);
        }
        if self.file.is_none() {
            self.progress.set_current(&self.name);
            self.file = Some(File::open(&self.path)?);
        }
        let n = self.file.as_mut().unwrap().read(buf)?;
        if n == 0 {
            self.file = None;
            self.finished = true;
        }
        self.progress.add(n as u64);
        Ok(n)
    }
}

/// Threads for the multithreaded encoders.
pub fn cpu_threads() -> u32 {
    std::thread::available_parallelism().map_or(2, |n| n.get() as u32)
}

#[cfg(test)]
mod tests;
