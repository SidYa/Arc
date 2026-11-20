//! Deflate on every core, the way pigz does it: the input is cut into chunks, each one is compressed
//! by its own thread (primed with the end of the chunk before it, so little compression is lost)
//! and ends on a byte boundary; written one after another, the pieces are one ordinary deflate
//! stream that any unpacker reads. ZIP entries and .gz files are made of it.

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use flate2::{Compress, Compression, FlushCompress, Status};

/// Input bytes per chunk
pub const CHUNK: usize = 1 << 20;
/// How far back deflate looks: this much of the previous chunk primes the next one
const WINDOW: usize = 32 << 10;

struct Job<T> {
    seq: u64,
    data: Vec<u8>,
    /// The end of the chunk before, when this one continues the same stream
    dict: Vec<u8>,
    /// The stream ends with this chunk
    last: bool,
    out: Vec<u8>,
    tag: T,
}

/// A compressed chunk, handed out in the order the chunks were submitted.
pub struct Done<T> {
    pub tag: T,
    /// What was submitted
    pub data: Vec<u8>,
    pub packed: Vec<u8>,
    /// CRC-32 of `data`
    pub crc: u32,
    pub last: bool,
}

pub struct Pool<T: Send + 'static> {
    jobs: Option<Sender<Job<T>>>,
    results: Receiver<(u64, io::Result<Done<T>>)>,
    workers: Vec<JoinHandle<()>>,
    /// Finished out of turn
    waiting: BTreeMap<u64, Done<T>>,
    submitted: u64,
    delivered: u64,
    /// Chunks in the works at a time: bounds the memory
    limit: u64,
    /// End of the last chunk, while its stream goes on
    tail: Vec<u8>,
    spare: Vec<Vec<u8>>,
    spare_out: Vec<Vec<u8>>,
}

fn other(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}

fn pack<T>(compress: &mut Compress, job: &mut Job<T>) -> io::Result<()> {
    compress.reset();
    if !job.dict.is_empty() {
        compress.set_dictionary(&job.dict).map_err(other)?;
    }
    let out = &mut job.out;
    out.clear();
    out.reserve(job.data.len() / 2 + 4096);
    let flush = if job.last { FlushCompress::Finish } else { FlushCompress::Sync };
    loop {
        let consumed = compress.total_in() as usize;
        let status = compress.compress_vec(&job.data[consumed..], out, flush).map_err(other)?;
        let done = if job.last {
            status == Status::StreamEnd
        } else {
            // A flush is complete when it didn't need all the room it had
            compress.total_in() as usize == job.data.len() && out.len() < out.capacity()
        };
        if done {
            return Ok(());
        }
        if out.len() == out.capacity() {
            out.reserve(out.capacity().max(64 << 10));
        }
    }
}

impl<T: Send + 'static> Pool<T> {
    /// `level` 0 (stored blocks) … 9
    pub fn new(threads: usize, level: u32) -> Pool<T> {
        let threads = threads.max(1);
        let (jobs, queue) = channel::<Job<T>>();
        let (finished, results) = channel();
        let queue = Arc::new(Mutex::new(queue));
        let workers = (0..threads)
            .map(|_| {
                let queue = queue.clone();
                let finished = finished.clone();
                std::thread::spawn(move || {
                    let mut compress = Compress::new(Compression::new(level.min(9)), false);
                    loop {
                        let job = queue.lock().unwrap().recv();
                        let Ok(mut job) = job else { return };
                        let result = pack(&mut compress, &mut job).map(|_| Done {
                            crc: crc32fast::hash(&job.data),
                            tag: job.tag,
                            data: job.data,
                            packed: job.out,
                            last: job.last,
                        });
                        if finished.send((job.seq, result)).is_err() {
                            return;
                        }
                    }
                })
            })
            .collect();
        Pool {
            jobs: Some(jobs),
            results,
            workers,
            waiting: BTreeMap::new(),
            submitted: 0,
            delivered: 0,
            limit: threads as u64 * 3,
            tail: Vec::new(),
            spare: Vec::new(),
            spare_out: Vec::new(),
        }
    }

    /// An empty buffer for the next chunk (a used one when there is any).
    pub fn buffer(&mut self) -> Vec<u8> {
        let mut buf = self.spare.pop().unwrap_or_default();
        buf.clear();
        buf.reserve(CHUNK);
        buf
    }

    /// Queues a chunk. `last` ends the stream: the next chunk starts a new one. Finished chunks
    /// go to `sink` in their order; this waits for them when too many are in the works.
    pub fn submit(&mut self, data: Vec<u8>, last: bool, tag: T, sink: &mut dyn FnMut(&mut Done<T>) -> io::Result<()>) -> io::Result<()> {
        if data.is_empty() && !last {
            self.spare.push(data);
            return Ok(());
        }
        let dict = std::mem::take(&mut self.tail);
        if !last {
            self.tail = data[data.len().saturating_sub(WINDOW)..].to_vec();
        }
        let job = Job { seq: self.submitted, data, dict, last, out: self.spare_out.pop().unwrap_or_default(), tag };
        self.jobs.as_ref().unwrap().send(job).map_err(|_| other("compression thread stopped"))?;
        self.submitted += 1;
        while self.submitted - self.delivered >= self.limit {
            self.deliver(sink)?;
        }
        Ok(())
    }

    /// Waits for everything submitted.
    pub fn finish(&mut self, sink: &mut dyn FnMut(&mut Done<T>) -> io::Result<()>) -> io::Result<()> {
        while self.delivered < self.submitted {
            self.deliver(sink)?;
        }
        Ok(())
    }

    /// The next chunk in order goes to `sink`.
    fn deliver(&mut self, sink: &mut dyn FnMut(&mut Done<T>) -> io::Result<()>) -> io::Result<()> {
        let mut done = loop {
            if let Some(done) = self.waiting.remove(&self.delivered) {
                break done;
            }
            let (seq, result) = self.results.recv().map_err(|_| other("compression thread stopped"))?;
            self.waiting.insert(seq, result?);
        };
        self.delivered += 1;
        sink(&mut done)?;
        self.spare.push(done.data);
        self.spare_out.push(done.packed);
        Ok(())
    }
}

impl<T: Send + 'static> Drop for Pool<T> {
    fn drop(&mut self) {
        // The threads leave when the queue closes
        self.jobs = None;
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

/// Folds the CRC of the next `len` bytes into the CRC of everything before them.
pub fn crc_append(total: &mut crc32fast::Hasher, crc: u32, len: usize) {
    total.combine(&crc32fast::Hasher::new_with_initial_len(crc, len as u64));
}

/// A gzip file written by all cores.
pub struct GzWriter<W: Write> {
    out: W,
    pool: Pool<()>,
    buf: Vec<u8>,
    crc: crc32fast::Hasher,
    size: u64,
}

impl<W: Write> GzWriter<W> {
    pub fn new(mut out: W, level: u32, threads: usize) -> io::Result<GzWriter<W>> {
        // Deflate, no name or time, unknown system
        out.write_all(&[0x1F, 0x8B, 8, 0, 0, 0, 0, 0, 0, 0xFF])?;
        let mut pool = Pool::new(threads, level);
        let buf = pool.buffer();
        Ok(GzWriter { out, pool, buf, crc: crc32fast::Hasher::new(), size: 0 })
    }

    fn send(&mut self, last: bool) -> io::Result<()> {
        let next = self.pool.buffer();
        let data = std::mem::replace(&mut self.buf, next);
        let (out, crc, size) = (&mut self.out, &mut self.crc, &mut self.size);
        self.pool.submit(data, last, (), &mut |done| Self::write_out(out, crc, size, done))?;
        if last {
            self.pool.finish(&mut |done| Self::write_out(out, crc, size, done))?;
        }
        Ok(())
    }

    fn write_out(out: &mut W, crc: &mut crc32fast::Hasher, size: &mut u64, done: &mut Done<()>) -> io::Result<()> {
        crc_append(crc, done.crc, done.data.len());
        *size += done.data.len() as u64;
        out.write_all(&done.packed)
    }

    /// Ends the stream and gives the writer back.
    pub fn finish(mut self) -> io::Result<W> {
        self.send(true)?;
        let crc = self.crc.clone().finalize();
        self.out.write_all(&crc.to_le_bytes())?;
        // The size modulo 4 GB, as the format has it
        self.out.write_all(&(self.size as u32).to_le_bytes())?;
        let GzWriter { out, pool, .. } = self;
        drop(pool);
        Ok(out)
    }
}

impl<W: Write> Write for GzWriter<W> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        let n = data.len().min(CHUNK - self.buf.len());
        self.buf.extend_from_slice(&data[..n]);
        if self.buf.len() == CHUNK {
            self.send(false)?;
        }
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
