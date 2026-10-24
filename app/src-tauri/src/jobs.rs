//! Background operations: create, extract, open a file from an archive.
//! One heavy job runs at a time (the disk is the bottleneck); opening a single file doesn't wait.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_opener::OpenerExt;

use crate::archive::{self, Conflict, CreateOptions, Error, ExtractOptions, Progress};

#[derive(Deserialize, Clone, Copy, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ExtractMode {
    /// Into `dest` as given
    #[default]
    To,
    /// Next to the archive
    Here,
    /// Into a folder named after the archive, next to it
    Folder,
}

#[derive(Deserialize, Clone)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum JobSpec {
    Create {
        sources: Vec<String>,
        dest: String,
        options: CreateOptions,
    },
    Extract {
        archive: String,
        #[serde(default)]
        dest: String,
        #[serde(default)]
        mode: ExtractMode,
        password: Option<String>,
        /// Only these entries; everything when absent
        items: Option<Vec<String>>,
        /// Folder in the archive the items are in
        #[serde(default)]
        base: String,
    },
    Open {
        archive: String,
        password: Option<String>,
        item: String,
    },
}

#[derive(Serialize, Clone)]
struct ProgressEvent {
    id: u32,
    done: u64,
    total: u64,
    current: String,
}

#[derive(Serialize, Clone)]
struct FinishedEvent {
    id: u32,
    ok: bool,
    /// Error code (password_required, wrong_password, cancelled) or message
    error: Option<String>,
    /// Created archive, extraction folder or opened file
    output: Option<String>,
}

#[derive(Default)]
pub struct Jobs {
    running: Mutex<HashMap<u32, Arc<Progress>>>,
    /// Held by the heavy job that is running
    worker: Arc<Mutex<()>>,
}

impl Jobs {
    pub fn cancel(&self, id: u32) {
        if let Some(p) = self.running.lock().unwrap().get(&id) {
            p.cancel();
        }
    }

    pub fn pause(&self, id: u32, paused: bool) {
        if let Some(p) = self.running.lock().unwrap().get(&id) {
            p.set_paused(paused);
        }
    }

    pub fn cancel_all(&self) {
        for p in self.running.lock().unwrap().values() {
            p.cancel();
        }
    }

    pub fn is_busy(&self) -> bool {
        !self.running.lock().unwrap().is_empty()
    }
}

pub fn start(app: &AppHandle, id: u32, spec: JobSpec, conflict: Conflict) {
    let jobs = app.state::<crate::AppState>();
    let progress = Arc::new(Progress::default());
    jobs.jobs.running.lock().unwrap().insert(id, progress.clone());
    let worker = jobs.jobs.worker.clone();
    let app = app.clone();
    std::thread::spawn(move || {
        let result = {
            // Opening one file shouldn't wait behind a long extraction
            let _turn = (!matches!(spec, JobSpec::Open { .. })).then(|| worker.lock().unwrap_or_else(|e| e.into_inner()));
            if progress.is_cancelled() { Err(Error::Cancelled) } else { run(&app, &spec, conflict, &progress) }
        };
        app.state::<crate::AppState>().jobs.running.lock().unwrap().remove(&id);
        if let Err(e) = &result
            && !matches!(e, Error::Cancelled | Error::PasswordRequired | Error::WrongPassword)
        {
            crate::log(&format!("Job {id} failed: {e}"));
        }
        let event = match result {
            Ok(output) => FinishedEvent { id, ok: true, error: None, output: Some(output.to_string_lossy().into_owned()) },
            Err(e) => FinishedEvent { id, ok: false, error: Some(e.to_string()), output: None },
        };
        let _ = app.emit("job-finished", event);
    });
}

fn run(app: &AppHandle, spec: &JobSpec, conflict: Conflict, progress: &Progress) -> archive::Result<PathBuf> {
    match spec {
        JobSpec::Create { sources, dest, options } => {
            let sources: Vec<PathBuf> = sources.iter().map(PathBuf::from).collect();
            let dest = PathBuf::from(dest);
            archive::create(&sources, &dest, options, progress)?;
            Ok(dest)
        }
        JobSpec::Extract { archive: path, dest, mode, password, items, base } => {
            let path = PathBuf::from(path);
            let parent = path.parent().map(Path::to_path_buf).unwrap_or_default();
            let items = items.as_ref().map(|i| i.iter().cloned().collect::<HashSet<_>>());
            match mode {
                ExtractMode::Folder => extract_to_folder(&path, &parent, password.as_deref(), items, base, progress),
                _ => {
                    let dest = if *mode == ExtractMode::Here { parent } else { PathBuf::from(dest) };
                    let opts = ExtractOptions { dest: &dest, password: password.as_deref(), items, base: base.clone(), conflict };
                    archive::extract(&path, &opts, progress)?;
                    Ok(dest)
                }
            }
        }
        JobSpec::Open { archive: path, password, item } => {
            let path = PathBuf::from(path);
            let dest = temp_folder(&path);
            let base = item.rsplit_once('/').map(|(dir, _)| dir.to_string()).unwrap_or_default();
            let opts = ExtractOptions {
                dest: &dest,
                password: password.as_deref(),
                items: Some(HashSet::from([item.clone()])),
                base,
                conflict: Conflict::Overwrite,
            };
            archive::extract(&path, &opts, progress)?;
            let name = item.rsplit('/').next().unwrap_or(item);
            let file = archive::safe_join(&dest, name).ok_or_else(|| Error::Other("unsafe_name".into()))?;
            if !file.exists() {
                return Err(Error::Other("not_found".into()));
            }
            app.opener().open_path(file.to_string_lossy(), None::<&str>).map_err(|e| Error::Other(e.to_string()))?;
            Ok(file)
        }
    }
}

/// Like 7-Zip's "Extract to <name>\", but "name.zip" holding just a "name" folder doesn't become
/// name\name: everything goes into a hidden temporary folder first, then is moved into place.
fn extract_to_folder(
    path: &Path,
    parent: &Path,
    password: Option<&str>,
    items: Option<HashSet<String>>,
    base: &str,
    progress: &Progress,
) -> archive::Result<PathBuf> {
    let stem = archive::archive_stem(path);
    let temp = archive::unique_path(&parent.join(format!(".{stem}.arc-tmp")));
    let opts = ExtractOptions { dest: &temp, password, items, base: base.to_string(), conflict: Conflict::Rename };
    if let Err(e) = archive::extract(path, &opts, progress) {
        let _ = fs::remove_dir_all(&temp);
        return Err(e);
    }
    let target = archive::unique_path(&parent.join(&stem));
    let children: Vec<_> = fs::read_dir(&temp)?.filter_map(|e| e.ok()).collect();
    if let [only] = children.as_slice()
        && only.path().is_dir()
        && only.file_name().to_string_lossy().to_lowercase() == stem.to_lowercase()
    {
        fs::rename(only.path(), &target)?;
        let _ = fs::remove_dir(&temp);
        return Ok(target);
    }
    fs::rename(&temp, &target)?;
    Ok(target)
}

/// A folder per archive, so files with the same name from different archives don't collide.
fn temp_folder(archive: &Path) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    archive.hash(&mut hasher);
    crate::config::temp_dir().join(format!("{:016x}", hasher.finish()))
}

/// Progress of all running jobs, ~7 times a second.
pub fn report_loop(app: AppHandle) {
    loop {
        std::thread::sleep(Duration::from_millis(150));
        let events: Vec<ProgressEvent> = {
            let state = app.state::<crate::AppState>();
            let running = state.jobs.running.lock().unwrap();
            running
                .iter()
                .map(|(id, p)| {
                    let (done, total) = p.snapshot();
                    ProgressEvent { id: *id, done, total, current: p.current.lock().unwrap().clone() }
                })
                .collect()
        };
        if !events.is_empty() {
            let _ = app.emit("job-progress", events);
        }
    }
}
