//! Explorer starts Arc once per selected file for a menu verb, one process after another.
//! They are merged here, before Tauri starts: every process appends its paths to a spool file,
//! the first one (the collector) waits until no more arrive and takes them all; the others exit
//! at once. This doesn't depend on how fast the first instance gets its window and event loop
//! going (which lost files when they were forwarded through the single-instance plugin).

use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, WAIT_ABANDONED, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject};
use windows::core::HSTRING;

/// No new file for this long: Explorer has started everything
const QUIET: Duration = Duration::from_millis(1000);
const MAX_WAIT: Duration = Duration::from_secs(60);

/// A named mutex held while touching the spool files.
struct Lock(HANDLE);

impl Lock {
    fn take() -> Option<Lock> {
        unsafe {
            let handle = CreateMutexW(None, false, &HSTRING::from(r"Local\Arc.menu.lock")).ok()?;
            let wait = WaitForSingleObject(handle, 10_000);
            if wait == WAIT_OBJECT_0 || wait == WAIT_ABANDONED {
                Some(Lock(handle))
            } else {
                let _ = CloseHandle(handle);
                None
            }
        }
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        unsafe {
            let _ = ReleaseMutex(self.0);
            let _ = CloseHandle(self.0);
        }
    }
}

fn spool_path(action: &str) -> PathBuf {
    crate::config::data_dir().join(format!("menu-{action}.txt"))
}

/// All the paths of this menu action, if this process is the one to handle them;
/// None when another Arc process has taken them (this one should just exit).
pub fn gather(action: &str, paths: Vec<String>) -> Option<Vec<String>> {
    let spool = spool_path(action);
    let _ = std::fs::create_dir_all(crate::config::data_dir());

    let collector = {
        // Without the lock (should not happen) this process just does its own part
        let Some(_lock) = Lock::take() else { return Some(paths) };
        let collector = unsafe { CreateMutexW(None, true, &HSTRING::from(format!(r"Local\Arc.menu.{action}"))) };
        let Ok(collector) = collector else { return Some(paths) };
        let first = unsafe { GetLastError() } != ERROR_ALREADY_EXISTS;
        // The collector starts a fresh spool (anything left there is from a crashed run)
        let file = if first {
            std::fs::File::create(&spool)
        } else {
            std::fs::OpenOptions::new().create(true).append(true).open(&spool)
        };
        let written = file.and_then(|mut f| f.write_all(paths.iter().map(|p| format!("{p}\n")).collect::<String>().as_bytes()));
        if !first {
            unsafe {
                let _ = CloseHandle(collector);
            }
            // Couldn't hand the paths over: do them here
            return if written.is_ok() { None } else { Some(paths) };
        }
        collector
    };

    // Wait until Explorer has started all the processes
    let start = Instant::now();
    let mut size = file_size(&spool);
    let mut changed = Instant::now();
    while changed.elapsed() < QUIET && start.elapsed() < MAX_WAIT {
        std::thread::sleep(Duration::from_millis(50));
        let now = file_size(&spool);
        if now != size {
            size = now;
            changed = Instant::now();
        }
    }

    // Take the spool and give up the collector role in one step, so a process arriving
    // right now either got in or becomes the next collector
    let _lock = Lock::take();
    let text = std::fs::read_to_string(&spool).unwrap_or_default();
    let _ = std::fs::remove_file(&spool);
    unsafe {
        let _ = ReleaseMutex(collector);
        let _ = CloseHandle(collector);
    }
    let mut all: Vec<String> = Vec::new();
    for line in text.lines().chain(paths.iter().map(String::as_str)) {
        if !line.is_empty() && !all.iter().any(|p| p == line) {
            all.push(line.to_string());
        }
    }
    Some(all)
}

fn file_size(path: &PathBuf) -> u64 {
    std::fs::metadata(path).map_or(0, |m| m.len())
}

#[cfg(test)]
mod tests {
    /// Seven "processes" (threads here: the named mutexes work the same way) arriving one by one,
    /// slower than the old 450 ms batching: exactly one of them gets all seven paths.
    #[test]
    fn merges_staggered_launches() {
        let action = format!("test-{}", std::process::id());
        let handles: Vec<_> = (0..7)
            .map(|i| {
                let action = action.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(i * 300));
                    super::gather(&action, vec![format!(r"C:\file{i}.txt")])
                })
            })
            .collect();
        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        let taken: Vec<_> = results.into_iter().flatten().collect();
        assert_eq!(taken.len(), 1, "{taken:?}");
        let mut all = taken[0].clone();
        all.sort();
        assert_eq!(all, (0..7).map(|i| format!(r"C:\file{i}.txt")).collect::<Vec<_>>());
        assert!(!super::spool_path(&action).exists());
    }
}
