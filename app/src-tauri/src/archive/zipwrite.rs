//! Writing ZIP archives on every core. The data of all files goes through one pool of compressing
//! threads in chunks (deflate.rs), so many small files and one huge file are both packed in
//! parallel; this thread reads the files and writes the results in their order.
//! ZIP64 where sizes need it, AES-256 (WinZip AE-2) with a password, UTF-8 names.

use std::collections::VecDeque;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::mpsc::{Receiver, sync_channel};

use aes::cipher::{BlockCipherEncrypt, KeyInit};
use hmac::{Mac, SimpleHmacReset};
use sha1::Sha1;

use super::deflate::{CHUNK, Done, Pool, crc_append};
use super::{Progress, Result, Source};

const STORED: u16 = 0;
const DEFLATED: u16 = 8;
/// The real method is in the AES extra field
const AES_METHOD: u16 = 99;
const FLAG_ENCRYPTED: u16 = 1;
const FLAG_UTF8: u16 = 0x0800;
/// Salt, password check and authentication code around the data of an encrypted file
const AES_OVERHEAD: u64 = 16 + 2 + 10;

fn put16(v: &mut Vec<u8>, x: u16) {
    v.extend_from_slice(&x.to_le_bytes());
}

fn put32(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&x.to_le_bytes());
}

fn put64(v: &mut Vec<u8>, x: u64) {
    v.extend_from_slice(&x.to_le_bytes());
}

/// AES-256 in the counter mode of WinZip (a little-endian counter from 1), with the HMAC-SHA1
/// of the encrypted data.
struct Crypt {
    cipher: aes::Aes256,
    counter: u128,
    /// Key stream of the last block that data didn't use up
    pad: [u8; 16],
    used: usize,
    hmac: SimpleHmacReset<Sha1>,
}

impl Crypt {
    /// The cipher, and what goes in front of the data: salt and password check.
    fn new(password: &str) -> io::Result<(Crypt, [u8; 18])> {
        let mut head = [0u8; 18];
        getrandom::fill(&mut head[..16]).map_err(|e| io::Error::other(e.to_string()))?;
        // Encryption key, authentication key, 2 bytes to tell a wrong password early
        let mut keys = [0u8; 66];
        pbkdf2::pbkdf2::<SimpleHmacReset<Sha1>>(password.as_bytes(), &head[..16], 1000, &mut keys)
            .map_err(|e| io::Error::other(e.to_string()))?;
        head[16..].copy_from_slice(&keys[64..]);
        let cipher = aes::Aes256::new_from_slice(&keys[..32]).map_err(|e| io::Error::other(e.to_string()))?;
        let hmac = SimpleHmacReset::<Sha1>::new_from_slice(&keys[32..64]).map_err(|e| io::Error::other(e.to_string()))?;
        Ok((Crypt { cipher, counter: 0, pad: [0; 16], used: 16, hmac }, head))
    }

    fn encrypt(&mut self, data: &mut [u8]) {
        let mut start = 0;
        while self.used < 16 && start < data.len() {
            data[start] ^= self.pad[self.used];
            self.used += 1;
            start += 1;
        }
        // Eight blocks at a time: the processor's AES instructions work on them side by side
        let mut blocks = [aes::Block::default(); 8];
        for piece in data[start..].chunks_mut(8 * 16) {
            let count = piece.len().div_ceil(16);
            for block in &mut blocks[..count] {
                self.counter += 1;
                block.copy_from_slice(&self.counter.to_le_bytes());
            }
            self.cipher.encrypt_blocks(&mut blocks[..count]);
            for (bytes, block) in piece.chunks_mut(16).zip(&blocks) {
                for (byte, key) in bytes.iter_mut().zip(block.iter()) {
                    *byte ^= key;
                }
            }
            if piece.len() % 16 != 0 {
                self.pad.copy_from_slice(&blocks[count - 1]);
                self.used = piece.len() % 16;
            }
        }
        self.hmac.update(data);
    }

    /// Authentication code that ends the file's data
    fn finish(self) -> [u8; 10] {
        let mut code = [0u8; 10];
        code.copy_from_slice(&self.hmac.finalize().into_bytes()[..10]);
        code
    }
}

type Key = io::Result<(Crypt, [u8; 18])>;

/// Every file has its own key, a thousand rounds of hashing each: with thousands of small files
/// that is most of the work, so threads make the keys ahead of the writer.
fn make_keys(password: &str, files: usize) -> Receiver<Key> {
    let (sender, keys) = sync_channel(64);
    let left = Arc::new(AtomicIsize::new(files as isize));
    for _ in 0..(super::cpu_threads() as usize).min(4).min(files) {
        let (sender, left, password) = (sender.clone(), left.clone(), password.to_string());
        std::thread::spawn(move || {
            while left.fetch_sub(1, Ordering::SeqCst) > 0 {
                // Nobody is waiting any more: the archive failed or was cancelled
                if sender.send(Crypt::new(&password)).is_err() {
                    return;
                }
            }
        });
    }
    keys
}

/// What the central directory says about a written entry.
struct Entry {
    name: String,
    is_dir: bool,
    method: u16,
    encrypted: bool,
    time: u16,
    date: u16,
    crc: u32,
    packed: u64,
    size: u64,
    offset: u64,
    /// The local header has 64-bit sizes
    zip64: bool,
}

impl Entry {
    fn flags(&self) -> u16 {
        (if self.encrypted { FLAG_ENCRYPTED } else { 0 }) | (if self.name.is_ascii() { 0 } else { FLAG_UTF8 })
    }

    fn version(&self, zip64: bool) -> u16 {
        if self.encrypted {
            51
        } else if zip64 {
            45
        } else {
            20
        }
    }

    fn aes_extra(&self, v: &mut Vec<u8>) {
        if self.encrypted {
            put16(v, 0x9901);
            put16(v, 7);
            // AE-2: no CRC of the plain data in the headers, the authentication code covers it
            put16(v, 2);
            v.extend_from_slice(b"AE");
            // AES-256
            v.push(3);
            put16(v, self.method);
        }
    }

    fn local_header(&self) -> Vec<u8> {
        let mut extra = Vec::new();
        if self.zip64 {
            put16(&mut extra, 1);
            put16(&mut extra, 16);
            put64(&mut extra, self.size);
            put64(&mut extra, self.packed);
        }
        self.aes_extra(&mut extra);
        let mut v = Vec::with_capacity(30 + self.name.len() + extra.len());
        put32(&mut v, 0x0403_4B50);
        put16(&mut v, self.version(self.zip64));
        put16(&mut v, self.flags());
        put16(&mut v, if self.encrypted { AES_METHOD } else { self.method });
        put16(&mut v, self.time);
        put16(&mut v, self.date);
        put32(&mut v, self.crc);
        put32(&mut v, if self.zip64 { u32::MAX } else { self.packed as u32 });
        put32(&mut v, if self.zip64 { u32::MAX } else { self.size as u32 });
        put16(&mut v, self.name.len() as u16);
        put16(&mut v, extra.len() as u16);
        v.extend_from_slice(self.name.as_bytes());
        v.extend_from_slice(&extra);
        v
    }

    fn central_header(&self, v: &mut Vec<u8>, limit: u64) {
        let big_sizes = self.zip64 || self.size >= limit || self.packed >= limit;
        let big_offset = self.offset >= limit;
        let mut extra = Vec::new();
        if big_sizes || big_offset {
            put16(&mut extra, 1);
            put16(&mut extra, (if big_sizes { 16 } else { 0 }) + (if big_offset { 8 } else { 0 }));
            if big_sizes {
                put64(&mut extra, self.size);
                put64(&mut extra, self.packed);
            }
            if big_offset {
                put64(&mut extra, self.offset);
            }
        }
        self.aes_extra(&mut extra);
        put32(v, 0x0201_4B50);
        // Made by: version 6.3 of the format, FAT attributes
        put16(v, 63);
        put16(v, self.version(big_sizes || big_offset));
        put16(v, self.flags());
        put16(v, if self.encrypted { AES_METHOD } else { self.method });
        put16(v, self.time);
        put16(v, self.date);
        put32(v, self.crc);
        put32(v, if big_sizes { u32::MAX } else { self.packed as u32 });
        put32(v, if big_sizes { u32::MAX } else { self.size as u32 });
        put16(v, self.name.len() as u16);
        put16(v, extra.len() as u16);
        // Comment, disk, internal attributes
        put16(v, 0);
        put16(v, 0);
        put16(v, 0);
        // Folder or archive attribute
        put32(v, if self.is_dir { 0x10 } else { 0x20 });
        put32(v, if big_offset { u32::MAX } else { self.offset as u32 });
        v.extend_from_slice(self.name.as_bytes());
        v.extend_from_slice(&extra);
    }
}

/// MS-DOS date and time (local, 2-second steps): what ZIP stores.
fn dos_time(source: &Source) -> (u16, u16) {
    use chrono::{Datelike, Local, Timelike};
    let local: chrono::DateTime<Local> = source.modified.into();
    if local.year() < 1980 {
        return ((1 << 5) | 1, 0);
    }
    let year = (local.year().min(2107) - 1980) as u16;
    let date = (year << 9) | ((local.month() as u16) << 5) | local.day() as u16;
    let time = ((local.hour() as u16) << 11) | ((local.minute() as u16) << 5) | (local.second() as u16 / 2);
    (date, time)
}

/// Which chunk of which file a piece of work is.
#[derive(Clone, Copy)]
struct Tag {
    index: usize,
    first: bool,
}

/// A file whose data is being written: its header went out before its sizes were known.
struct Open {
    entry: Entry,
    crc: crc32fast::Hasher,
    crypt: Option<Crypt>,
}

struct Writer<'a, W: Write + Seek> {
    out: W,
    /// Bytes written: where the next header goes
    pos: u64,
    sources: &'a [Source],
    password: Option<&'a str>,
    keys: Option<Receiver<Key>>,
    /// Values from here on take the 64-bit fields (4 GB, less in tests)
    limit: u64,
    entries: Vec<Entry>,
    open: Option<Open>,
    /// Sources handed to the pool and not written yet, with the folders that come after them
    order: VecDeque<usize>,
}

impl<W: Write + Seek> Writer<'_, W> {
    fn write(&mut self, data: &[u8]) -> io::Result<()> {
        self.out.write_all(data)?;
        self.pos += data.len() as u64;
        Ok(())
    }

    fn entry(&self, index: usize, method: u16) -> io::Result<Entry> {
        let source = &self.sources[index];
        let name = if source.is_dir { format!("{}/", source.name) } else { source.name.clone() };
        if name.len() > u16::MAX as usize {
            return Err(io::Error::other(format!("name too long: {name}")));
        }
        let (date, time) = dos_time(source);
        Ok(Entry {
            name,
            is_dir: source.is_dir,
            method,
            encrypted: self.password.is_some() && !source.is_dir,
            time,
            date,
            crc: 0,
            packed: 0,
            size: 0,
            offset: self.pos,
            zip64: false,
        })
    }

    /// The cipher of the next file
    fn crypt(&mut self) -> Key {
        match self.keys.as_ref().map(|keys| keys.recv()) {
            Some(Ok(key)) => key,
            _ => Crypt::new(self.password.unwrap_or_default()),
        }
    }

    fn directory(&mut self, index: usize) -> io::Result<()> {
        let entry = self.entry(index, STORED)?;
        self.write(&entry.local_header())?;
        self.entries.push(entry);
        Ok(())
    }

    /// A folder is written in its turn: after the files before it that are still being packed.
    fn add_directory(&mut self, index: usize) -> io::Result<()> {
        if self.order.is_empty() {
            return self.directory(index);
        }
        self.order.push_back(index);
        Ok(())
    }

    /// The next chunk of a file, in order. `store`: the data goes in as it is.
    fn accept(&mut self, done: &mut Done<Tag>, store: bool) -> io::Result<()> {
        let Tag { index, first } = done.tag;
        if first && done.last {
            // The whole file in one piece: everything is known before its header is written.
            // What deflate cannot make smaller is stored
            let store = store || done.packed.len() >= done.data.len();
            let mut entry = self.entry(index, if store { STORED } else { DEFLATED })?;
            entry.size = done.data.len() as u64;
            let data = if store { &mut done.data } else { &mut done.packed };
            let mut around = None;
            if entry.encrypted {
                let (mut crypt, head) = self.crypt()?;
                crypt.encrypt(data);
                around = Some((head, crypt.finish()));
            } else {
                entry.crc = done.crc;
            }
            let data = if store { &done.data } else { &done.packed };
            entry.packed = data.len() as u64 + if around.is_some() { AES_OVERHEAD } else { 0 };
            self.write(&entry.local_header())?;
            match around {
                Some((head, code)) => {
                    self.write(&head)?;
                    self.write(data)?;
                    self.write(&code)?;
                }
                None => self.write(data)?,
            }
            self.entries.push(entry);
            return self.completed();
        }

        if first {
            let mut entry = self.entry(index, if store { STORED } else { DEFLATED })?;
            // Sizes that may not fit 32 bits (packing can add a little) get the 64-bit fields
            entry.zip64 = self.sources[index].size >= self.limit - self.limit / 16;
            self.write(&entry.local_header())?;
            let mut crypt = None;
            if entry.encrypted {
                let (c, head) = self.crypt()?;
                self.write(&head)?;
                entry.packed = AES_OVERHEAD;
                crypt = Some(c);
            }
            self.open = Some(Open { entry, crc: crc32fast::Hasher::new(), crypt });
        }
        let mut open = self.open.take().ok_or_else(|| io::Error::other("zip: chunk without a file"))?;
        crc_append(&mut open.crc, done.crc, done.data.len());
        open.entry.size += done.data.len() as u64;
        let data = if open.entry.method == STORED { &mut done.data } else { &mut done.packed };
        if let Some(crypt) = &mut open.crypt {
            crypt.encrypt(data);
        }
        open.entry.packed += data.len() as u64;
        self.write(data)?;
        if !done.last {
            self.open = Some(open);
            return Ok(());
        }

        let Open { mut entry, crc, crypt } = open;
        match crypt {
            Some(crypt) => self.write(&crypt.finish())?,
            None => entry.crc = crc.finalize(),
        }
        if !entry.zip64 && (entry.size >= self.limit || entry.packed >= self.limit) {
            return Err(io::Error::other(format!("file grew while it was archived: {}", entry.name)));
        }
        // Back to the header for the sizes
        self.out.seek(SeekFrom::Start(entry.offset))?;
        self.out.write_all(&entry.local_header())?;
        self.out.seek(SeekFrom::Start(self.pos))?;
        self.entries.push(entry);
        self.completed()
    }

    /// A file is done: the folders waiting behind it can go out.
    fn completed(&mut self) -> io::Result<()> {
        self.order.pop_front();
        while let Some(&index) = self.order.front() {
            if !self.sources[index].is_dir {
                break;
            }
            self.order.pop_front();
            self.directory(index)?;
        }
        Ok(())
    }

    fn finish(mut self) -> io::Result<W> {
        let start = self.pos;
        let mut directory = Vec::new();
        let entries = std::mem::take(&mut self.entries);
        for entry in &entries {
            entry.central_header(&mut directory, self.limit);
            if directory.len() >= 1 << 20 {
                self.write(&directory)?;
                directory.clear();
            }
        }
        self.write(&directory)?;
        let size = self.pos - start;
        let count = entries.len() as u64;
        let mut end = Vec::new();
        if count >= 0xFFFF || size >= self.limit || start >= self.limit {
            let record = self.pos;
            put32(&mut end, 0x0606_4B50);
            put64(&mut end, 44);
            put16(&mut end, 63);
            put16(&mut end, 45);
            put32(&mut end, 0);
            put32(&mut end, 0);
            put64(&mut end, count);
            put64(&mut end, count);
            put64(&mut end, size);
            put64(&mut end, start);
            // Where that record is
            put32(&mut end, 0x0706_4B50);
            put32(&mut end, 0);
            put64(&mut end, record);
            put32(&mut end, 1);
        }
        put32(&mut end, 0x0605_4B50);
        put16(&mut end, 0);
        put16(&mut end, 0);
        put16(&mut end, count.min(0xFFFF) as u16);
        put16(&mut end, count.min(0xFFFF) as u16);
        put32(&mut end, if size >= self.limit { u32::MAX } else { size as u32 });
        put32(&mut end, if start >= self.limit { u32::MAX } else { start as u32 });
        put16(&mut end, 0);
        self.write(&end)?;
        self.out.flush()?;
        Ok(self.out)
    }
}

/// Reads until the buffer has a whole chunk or the file ends.
fn read_chunk(file: &mut File, buf: &mut Vec<u8>, progress: &Progress) -> io::Result<()> {
    buf.clear();
    progress.check()?;
    // Straight into the spare room of the buffer: it isn't cleared first (a megabyte per small file)
    let read = Read::by_ref(file).take(CHUNK as u64).read_to_end(buf)?;
    progress.add(read as u64);
    Ok(())
}

pub fn create<W: Write + Seek>(out: W, sources: &[Source], level: u32, password: Option<&str>, progress: &Progress) -> Result<()> {
    create_with(out, sources, level, password, progress, u32::MAX as u64).map(drop)
}

/// `limit`: values from it on are written in the 64-bit fields (tests lower it to reach them).
pub(super) fn create_with<W: Write + Seek>(
    out: W,
    sources: &[Source],
    level: u32,
    password: Option<&str>,
    progress: &Progress,
    limit: u64,
) -> Result<W> {
    let keys = password.map(|p| make_keys(p, sources.iter().filter(|s| !s.is_dir).count()));
    let mut writer = Writer {
        out,
        pos: 0,
        sources,
        password,
        keys,
        limit,
        entries: Vec::with_capacity(sources.len()),
        open: None,
        order: VecDeque::new(),
    };
    // Storing needs no threads: it is as fast as the disks
    let mut pool: Option<Pool<Tag>> = (level > 0).then(|| Pool::new(super::cpu_threads() as usize, level));
    let mut spare = Vec::new();
    for (index, source) in sources.iter().enumerate() {
        progress.check()?;
        if source.is_dir {
            writer.add_directory(index)?;
            continue;
        }
        progress.set_current(&source.name);
        let mut file = File::open(&source.path)?;
        writer.order.push_back(index);
        let mut first = true;
        loop {
            let mut data = match &mut pool {
                Some(pool) => pool.buffer(),
                None => std::mem::take(&mut spare),
            };
            read_chunk(&mut file, &mut data, progress)?;
            let last = data.len() < CHUNK;
            let tag = Tag { index, first };
            match &mut pool {
                Some(pool) => pool.submit(data, last, tag, &mut |done| writer.accept(done, false))?,
                None => {
                    let mut done = Done { tag, crc: crc32fast::hash(&data), data, packed: Vec::new(), last };
                    writer.accept(&mut done, true)?;
                    spare = done.data;
                }
            }
            first = false;
            if last {
                break;
            }
        }
    }
    if let Some(pool) = &mut pool {
        pool.finish(&mut |done| writer.accept(done, false))?;
    }
    drop(pool);
    Ok(writer.finish()?)
}
