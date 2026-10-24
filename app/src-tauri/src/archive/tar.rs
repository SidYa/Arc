//! tar, tar.gz/.tgz, tar.xz/.txz, and single-file .gz/.xz. These are streams:
//! progress follows the compressed bytes read, since the unpacked size is unknown up front.

use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::Path;

use flate2::Compression;
use flate2::read::MultiGzDecoder;
use flate2::write::GzEncoder;
use tar::{Archive, Builder, EntryType, Header};

use super::{Counted, Entry, Error, ExtractOptions, Format, Listing, Progress, Result, Source};

/// Decompressed stream of the archive; reading it advances the progress by compressed bytes.
fn open<'a>(path: &Path, format: Format, progress: &'a Progress) -> Result<Box<dyn Read + 'a>> {
    let file = File::open(path)?;
    progress.set_total(file.metadata()?.len());
    let raw = Counted::new(BufReader::with_capacity(1 << 20, file), progress);
    Ok(match format {
        Format::TarGz | Format::Gz => Box::new(MultiGzDecoder::new(raw)),
        Format::TarXz | Format::Xz => Box::new(liblzma::read::XzDecoder::new_parallel(raw)),
        _ => Box::new(raw),
    })
}

/// Name of the single file inside a .gz/.xz: the archive name without the extension.
fn single_name(path: &Path) -> String {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let lower = name.to_lowercase();
    for ext in [".gz", ".xz"] {
        if lower.ends_with(ext) && name.len() > ext.len() {
            return name[..name.len() - ext.len()].to_string();
        }
    }
    format!("{name}.out")
}

/// A .gz may still hold a tarball under another name: look at the "ustar" magic.
fn effective_format(path: &Path, format: Format) -> Format {
    if !matches!(format, Format::Gz | Format::Xz | Format::TarGz | Format::TarXz) {
        return format;
    }
    let probe = Progress::default();
    let is_tar = open(path, format, &probe)
        .ok()
        .and_then(|mut r| {
            let mut block = [0u8; 512];
            r.read_exact(&mut block).ok()?;
            Some(&block[257..262] == b"ustar")
        })
        .unwrap_or(false);
    match (format, is_tar) {
        (Format::Gz | Format::TarGz, true) => Format::TarGz,
        (Format::Xz | Format::TarXz, true) => Format::TarXz,
        (Format::Gz | Format::TarGz, false) => Format::Gz,
        (_, false) => Format::Xz,
        (other, _) => other,
    }
}

fn map_io(e: io::Error, progress: &Progress) -> Error {
    if progress.is_cancelled() { Error::Cancelled } else { e.into() }
}

pub fn list(path: &Path, format: Format, progress: &Progress) -> Result<Listing> {
    let format = effective_format(path, format);
    if matches!(format, Format::Gz | Format::Xz) {
        // The size is only known after decompressing
        let mut stream = open(path, format, progress)?;
        let size = super::copy(&mut stream, &mut io::sink(), progress).map_err(|e| map_io(e, progress))?;
        let modified = fs::metadata(path).ok().and_then(|m| m.modified().ok()).and_then(|t| {
            t.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs() as i64)
        });
        let entry = Entry { path: single_name(path), is_dir: false, size, packed: 0, modified, encrypted: false };
        return Ok(Listing { format, entries: vec![entry], encrypted: false, solid: true, size: 0 });
    }

    let mut archive = Archive::new(open(path, format, progress)?);
    let mut entries = Vec::new();
    for entry in archive.entries().map_err(|e| map_io(e, progress))? {
        let entry = entry.map_err(|e| map_io(e, progress))?;
        let header = entry.header();
        let kind = header.entry_type();
        if !(kind.is_file() || kind.is_dir() || kind == EntryType::Continuous) {
            continue;
        }
        let name = super::normalize(&String::from_utf8_lossy(&entry.path_bytes()));
        if name.is_empty() {
            continue;
        }
        entries.push(Entry {
            path: name,
            is_dir: kind.is_dir(),
            size: entry.size(),
            packed: 0,
            modified: header.mtime().ok().map(|t| t as i64),
            encrypted: false,
        });
    }
    Ok(Listing { format, entries, encrypted: false, solid: true, size: 0 })
}

pub fn extract(path: &Path, format: Format, opts: &ExtractOptions, progress: &Progress) -> Result<()> {
    let format = effective_format(path, format);
    if matches!(format, Format::Gz | Format::Xz) {
        let name = single_name(path);
        let Some(relative) = opts.target(&name) else { return Ok(()) };
        let mut stream = open(path, format, progress)?;
        progress.set_current(&relative);
        let Some((target, file)) = super::create_file(opts, &relative)? else { return Ok(()) };
        let mut out = BufWriter::with_capacity(1 << 20, file);
        // Progress counts the compressed input here: write to a plain sink of counts
        let result = copy_uncounted(&mut stream, &mut out, progress).and_then(|_| out.flush());
        drop(out);
        if let Err(e) = result {
            let _ = fs::remove_file(&target);
            return Err(map_io(e, progress));
        }
        return Ok(());
    }

    let mut archive = Archive::new(open(path, format, progress)?);
    let mut dir_times = Vec::new();
    for entry in archive.entries().map_err(|e| map_io(e, progress))? {
        let mut entry = entry.map_err(|e| map_io(e, progress))?;
        let kind = entry.header().entry_type();
        let name = String::from_utf8_lossy(&entry.path_bytes()).into_owned();
        let Some(relative) = opts.target(&name) else { continue };
        let modified = entry.header().mtime().ok().map(|t| t as i64);
        progress.set_current(&relative);
        if kind.is_dir() {
            if let Some(dir) = opts.dir_path(&relative) {
                fs::create_dir_all(&dir)?;
                dir_times.push((dir, modified));
            }
        } else if kind.is_file() || kind == EntryType::Continuous {
            let Some((target, file)) = super::create_file(opts, &relative)? else { continue };
            let mut out = BufWriter::with_capacity(1 << 20, file);
            let result = copy_uncounted(&mut entry, &mut out, progress).and_then(|_| out.flush());
            drop(out);
            if let Err(e) = result {
                let _ = fs::remove_file(&target);
                return Err(map_io(e, progress));
            }
            super::set_modified(&target, modified);
        }
        // Links and devices are skipped: they need privileges on Windows and are a security risk
    }
    for (dir, modified) in dir_times {
        super::set_modified(&dir, modified);
    }
    Ok(())
}

/// Copy whose progress is counted on the compressed side (by `open`).
fn copy_uncounted(reader: &mut dyn Read, writer: &mut dyn Write, progress: &Progress) -> io::Result<u64> {
    let mut buf = vec![0u8; 256 * 1024];
    let mut total = 0;
    loop {
        progress.check()?;
        let n = reader.read(&mut buf)?;
        if n == 0 {
            return Ok(total);
        }
        writer.write_all(&buf[..n])?;
        total += n as u64;
    }
}

pub fn create<W: Write>(out: W, format: Format, sources: &[Source], level: u32, progress: &Progress) -> Result<()> {
    let level = level.min(9);
    match format {
        Format::TarGz => {
            let encoder = GzEncoder::new(out, Compression::new(level));
            let encoder = write_tar(encoder, sources, progress)?;
            encoder.finish()?.flush()?;
        }
        Format::TarXz => {
            // Parallel xz splits the stream into blocks: all cores, still readable by every xz tool
            let mut stream = liblzma::stream::MtStreamBuilder::new();
            stream.threads(super::cpu_threads().min(16)).preset(level).check(liblzma::stream::Check::Crc64);
            let stream = stream.encoder().map_err(|e| Error::Other(e.to_string()))?;
            let encoder = liblzma::write::XzEncoder::new_stream(out, stream);
            let encoder = write_tar(encoder, sources, progress)?;
            encoder.finish()?.flush()?;
        }
        _ => {
            let mut out = write_tar(out, sources, progress)?;
            out.flush()?;
        }
    }
    Ok(())
}

fn write_tar<W: Write>(out: W, sources: &[Source], progress: &Progress) -> Result<W> {
    let mut builder = Builder::new(out);
    builder.follow_symlinks(false);
    for source in sources {
        progress.check()?;
        progress.set_current(&source.name);
        let mut header = Header::new_gnu();
        header.set_mtime(source.unix_time().max(0) as u64);
        if source.is_dir {
            header.set_entry_type(EntryType::Directory);
            header.set_mode(0o755);
            header.set_size(0);
            builder
                .append_data(&mut header, format!("{}/", source.name), io::empty())
                .map_err(|e| map_io(e, progress))?;
        } else {
            header.set_entry_type(EntryType::Regular);
            header.set_mode(0o644);
            header.set_size(source.size);
            let file = Counted::new(BufReader::with_capacity(1 << 20, File::open(&source.path)?), progress);
            // A file that changed size while archiving would corrupt the tar: exactly the header size, zero-padded
            let reader = file.take(source.size).chain(io::repeat(0)).take(source.size);
            builder.append_data(&mut header, &source.name, reader).map_err(|e| map_io(e, progress))?;
        }
    }
    builder.into_inner().map_err(|e| map_io(e, progress))
}
