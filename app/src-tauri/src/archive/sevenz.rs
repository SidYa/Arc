//! 7z: LZMA2 (multithreaded), AES-256 with optional header (file names) encryption.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, BufReader, Seek, Write};
use std::path::Path;

use sevenz_rust2::encoder_options::{AesEncoderOptions, Lzma2Options};
use sevenz_rust2::{ArchiveEntry, ArchiveReader, ArchiveWriter, BlockDecoder, EncoderMethod, NtTime, Password, SourceReader};

use super::{Entry, Error, ExtractOptions, Format, LazyFile, Listing, Progress, Result, Source};

fn map_error(e: sevenz_rust2::Error, has_password: bool) -> Error {
    use sevenz_rust2::Error as E;
    match e {
        E::PasswordRequired => Error::PasswordRequired,
        E::MaybeBadPassword(_) => Error::WrongPassword,
        // Wrong key: the decrypted header or data is garbage
        E::ChecksumVerificationFailed | E::NextHeaderCrcMismatch | E::BadTerminatedHeader(_) if has_password => {
            Error::WrongPassword
        }
        E::Io(io, _) | E::FileOpen(io, _) => io.into(),
        other => Error::Other(other.to_string()),
    }
}

fn password(p: Option<&str>) -> Password {
    p.map(Password::new).unwrap_or_else(Password::empty)
}

fn unix_time(t: NtTime) -> Option<i64> {
    let time: std::time::SystemTime = t.into();
    time.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs() as i64)
}

fn open(path: &Path, pw: Option<&str>) -> Result<ArchiveReader<BufReader<File>>> {
    let file = BufReader::new(File::open(path)?);
    ArchiveReader::new(file, password(pw)).map_err(|e| map_error(e, pw.is_some()))
}

pub fn list(path: &Path, pw: Option<&str>) -> Result<Listing> {
    let reader = open(path, pw)?;
    // Data encryption is a coder of a block; header encryption shows up as PasswordRequired above
    let encrypted = reader.archive().blocks.iter().any(|b| b.coders.iter().any(|c| c.encoder_method_id() == EncoderMethod::ID_AES256_SHA256));
    let archive = reader.archive();
    let entries = archive
        .files
        .iter()
        .filter(|f| !f.is_anti_item())
        .map(|f| Entry {
            path: super::normalize(f.name()),
            is_dir: f.is_directory(),
            size: f.size(),
            packed: f.compressed_size,
            modified: if f.has_last_modified_date { unix_time(f.last_modified_date()) } else { None },
            encrypted,
        })
        .filter(|e| !e.path.is_empty())
        .collect();
    Ok(Listing { format: Format::SevenZ, entries, encrypted, solid: archive.is_solid, size: 0 })
}

pub fn extract(path: &Path, opts: &ExtractOptions, progress: &Progress) -> Result<()> {
    let has_password = opts.password.is_some();
    let reader = open(path, opts.password)?;
    let pw = password(opts.password);
    let archive = reader.archive().clone();
    let threads = super::cpu_threads().min(8);

    // Blocks holding a wanted file are decoded; the rest is skipped entirely
    let wanted: Vec<Option<String>> = archive.files.iter().map(|f| opts.target(f.name())).collect();
    let mut blocks = HashSet::new();
    let mut total = 0;
    for (index, _) in archive.files.iter().enumerate() {
        if wanted[index].is_some()
            && let Some(block) = archive.stream_map.file_block_index[index]
        {
            blocks.insert(block);
        }
    }
    for (index, file) in archive.files.iter().enumerate() {
        if archive.stream_map.file_block_index[index].is_some_and(|b| blocks.contains(&b)) {
            // Unwanted files of a solid block are decoded too
            total += file.size();
        }
    }
    progress.set_total(total);

    let source = reader_source(path)?;
    let mut source = source;
    let mut sorted: Vec<_> = blocks.into_iter().collect();
    sorted.sort_unstable();
    for block in sorted {
        let first = archive.stream_map.block_first_file_index[block];
        let mut decoder = BlockDecoder::new(threads, block, &archive, &pw, &mut source);
        decoder.set_thread_count(threads);
        let mut offset = first;
        decoder
            .for_each_entries(&mut |entry, data| {
                let index = offset;
                offset += 1;
                progress.check()?;
                match wanted.get(index).cloned().flatten() {
                    Some(relative) if !entry.is_directory() => {
                        progress.set_current(&relative);
                        match super::create_file(opts, &relative)? {
                            Some((target, file)) => {
                                let mut out = io::BufWriter::with_capacity(1 << 20, file);
                                let copied = super::copy(data, &mut out, progress).and_then(|_| out.flush());
                                drop(out);
                                if let Err(e) = copied {
                                    let _ = fs::remove_file(&target);
                                    return Err(e.into());
                                }
                                if entry.has_last_modified_date {
                                    super::set_modified(&target, unix_time(entry.last_modified_date()));
                                }
                            }
                            None => {
                                super::copy(data, &mut io::sink(), progress)?;
                            }
                        }
                    }
                    _ => {
                        super::copy(data, &mut io::sink(), progress)?;
                    }
                }
                Ok(true)
            })
            .map_err(|e| if progress.is_cancelled() { Error::Cancelled } else { map_error(e, has_password) })?;
    }
    drop(reader);

    // Folders and empty files have no data in any block
    for (index, file) in archive.files.iter().enumerate() {
        if archive.stream_map.file_block_index[index].is_some() {
            continue;
        }
        let Some(relative) = &wanted[index] else { continue };
        if file.is_directory() {
            if let Some(dir) = opts.dir_path(relative) {
                fs::create_dir_all(dir)?;
            }
        } else if !file.is_anti_item()
            && let Some((target, _)) = super::create_file(opts, relative)?
            && file.has_last_modified_date
        {
            super::set_modified(&target, unix_time(file.last_modified_date()));
        }
    }
    Ok(())
}

fn reader_source(path: &Path) -> Result<BufReader<File>> {
    Ok(BufReader::with_capacity(1 << 20, File::open(path)?))
}

pub fn create<W: Write + Seek>(
    out: W,
    sources: &[Source],
    level: u32,
    pw: Option<&str>,
    encrypt_names: bool,
    progress: &Progress,
) -> Result<()> {
    let mut writer = ArchiveWriter::new(out).map_err(|e| map_error(e, false))?;
    let level = level.min(9);
    // LZMA2 needs ~10x the dictionary per thread: fewer threads for the large dictionaries
    let threads = super::cpu_threads().min(match level {
        0..=5 => 8,
        6..=7 => 4,
        _ => 2,
    });
    let compression = if level == 0 {
        EncoderMethod::COPY.into()
    } else {
        Lzma2Options::from_level_mt(level, threads, 64 << 20).into()
    };
    let mut methods = Vec::new();
    if let Some(p) = pw {
        methods.push(AesEncoderOptions::new(Password::new(p)).into());
    }
    methods.push(compression);
    writer.set_content_methods(methods);
    writer.set_encrypt_header(pw.is_some() && encrypt_names);

    let entry = |s: &Source| {
        let mut e = if s.is_dir { ArchiveEntry::new_directory(&s.name) } else { ArchiveEntry::new_file(&s.name) };
        if let Ok(t) = NtTime::try_from(s.modified) {
            e.last_modified_date = t;
            e.has_last_modified_date = true;
        }
        e
    };

    for dir in sources.iter().filter(|s| s.is_dir) {
        writer.push_archive_entry::<File>(entry(dir), None).map_err(|e| map_error(e, false))?;
    }
    // Solid blocks of up to 4 GB / 10,000 files keep single-file extraction reasonable
    let files: Vec<&Source> = sources.iter().filter(|s| !s.is_dir).collect();
    let mut start = 0;
    while start < files.len() {
        let mut end = start;
        let mut bytes = 0u64;
        while end < files.len() && (end == start || (bytes + files[end].size <= 4 << 30 && end - start < 10_000)) {
            bytes += files[end].size;
            end += 1;
        }
        let chunk = &files[start..end];
        let entries = chunk.iter().map(|s| entry(s)).collect();
        let readers = chunk.iter().map(|s| SourceReader::new(LazyFile::new(s, progress))).collect();
        writer
            .push_archive_entries(entries, readers)
            .map_err(|e| if progress.is_cancelled() { Error::Cancelled } else { map_error(e, false) })?;
        start = end;
    }
    let mut out = writer.finish()?;
    out.flush()?;
    Ok(())
}
