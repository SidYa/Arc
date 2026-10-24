mod archive;
mod config;
mod jobs;
mod shell;

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};
use tauri_plugin_opener::OpenerExt;

use archive::{Conflict, Format, Listing};
use config::Config;
use jobs::{JobSpec, Jobs};

pub fn log(message: &str) {
    eprintln!("{message}");
    let dir = config::data_dir();
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("arc.log")) {
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let _ = writeln!(f, "{secs} {message}");
    }
}

/// What Arc was asked to do from the command line (Explorer menu, "Open with", a second launch).
#[derive(Serialize, Clone, Debug)]
struct OpenRequest {
    /// open | add | add_password | extract_here | extract_folder
    action: String,
    paths: Vec<String>,
}

fn parse_args(args: &[String], cwd: &Path) -> Option<OpenRequest> {
    let mut action = "open";
    let mut paths = Vec::new();
    for arg in args.iter().skip(1) {
        match arg.as_str() {
            "--add" => action = "add",
            "--add-password" => action = "add_password",
            "--extract-here" => action = "extract_here",
            "--extract-folder" => action = "extract_folder",
            a if a.starts_with("--") => {}
            path => {
                let path = PathBuf::from(path.trim_matches('"'));
                let path = if path.is_absolute() { path } else { cwd.join(path) };
                paths.push(path.to_string_lossy().into_owned());
            }
        }
    }
    (!paths.is_empty()).then(|| OpenRequest { action: action.into(), paths })
}

#[derive(Default)]
struct Batch {
    requests: Vec<OpenRequest>,
    deadline: Option<Instant>,
}

/// Work for a small progress window: a request from the Explorer menu, or jobs prepared by the main window.
#[derive(Serialize, Deserialize, Clone, Debug)]
struct Task {
    /// add | add_password | extract_here | extract_folder | jobs
    action: String,
    #[serde(default)]
    paths: Vec<String>,
    /// Ready job specs (action "jobs"), passed through to the window as they are
    #[serde(default)]
    specs: Option<serde_json::Value>,
}

#[derive(Default)]
pub struct AppState {
    config: Mutex<Config>,
    jobs: Jobs,
    pending_open: Mutex<Vec<OpenRequest>>,
    batch: Mutex<Batch>,
    batching: AtomicBool,
    /// Task windows take their work from here by the number in their label
    tasks: Mutex<HashMap<u32, Task>>,
    next_task: AtomicU32,
    quitting: AtomicBool,
}

impl AppState {
    fn conflict(&self) -> Conflict {
        match self.config.lock().unwrap().conflict.as_str() {
            "overwrite" => Conflict::Overwrite,
            "skip" => Conflict::Skip,
            _ => Conflict::Rename,
        }
    }
}

// ------------------------------------------------------------------ commands

#[derive(Serialize)]
struct InitialState {
    config: Config,
    version: String,
    platform: &'static str,
    system_accent: Option<String>,
}

#[tauri::command]
fn get_state(app: AppHandle, state: State<AppState>) -> InitialState {
    InitialState {
        config: state.config.lock().unwrap().clone(),
        version: app.package_info().version.to_string(),
        platform: if cfg!(windows) { "windows" } else if cfg!(target_os = "macos") { "macos" } else { "linux" },
        system_accent: shell::system_accent(),
    }
}

/// The work of the task window "task-<id>".
#[tauri::command]
fn take_task(state: State<AppState>, id: u32) -> Option<Task> {
    state.tasks.lock().unwrap().remove(&id)
}

/// The main window hands extraction and archiving to a progress window of their own.
#[tauri::command]
async fn open_task(app: AppHandle, task: Task) -> Result<(), String> {
    create_task_window(&app, task).map_err(|e| e.to_string())
}

#[tauri::command]
fn take_pending_open(state: State<AppState>) -> Vec<OpenRequest> {
    std::mem::take(&mut *state.pending_open.lock().unwrap())
}

#[tauri::command]
async fn list_archive(path: String, password: Option<String>) -> Result<Listing, String> {
    tauri::async_runtime::spawn_blocking(move || {
        archive::list(Path::new(&path), password.as_deref(), &archive::Progress::default()).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Serialize)]
struct PathInfo {
    path: String,
    name: String,
    is_dir: bool,
    /// Detected archive format, if any
    format: Option<Format>,
}

/// What the dropped or opened paths are.
#[tauri::command]
fn path_info(paths: Vec<String>) -> Vec<PathInfo> {
    paths
        .into_iter()
        .map(|p| {
            let path = PathBuf::from(&p);
            let is_dir = path.is_dir();
            PathInfo {
                name: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.clone()),
                format: if is_dir { None } else { Format::detect(&path) },
                is_dir,
                path: p,
            }
        })
        .collect()
}

/// A free archive name next to the sources: "report.zip" for one item, "<folder>.zip" for many.
#[tauri::command]
fn suggest_archive(sources: Vec<String>, format: Format) -> String {
    let first = PathBuf::from(sources.first().cloned().unwrap_or_default());
    let dir = first.parent().map(Path::to_path_buf).unwrap_or_default();
    let stem = if sources.len() == 1 {
        if first.is_dir() {
            first.file_name().map(|n| n.to_string_lossy().into_owned())
        } else {
            first.file_stem().map(|n| n.to_string_lossy().into_owned())
        }
    } else {
        dir.file_name().map(|n| n.to_string_lossy().into_owned())
    }
    .filter(|s| !s.is_empty())
    .unwrap_or_else(|| "archive".into());
    archive::unique_path(&dir.join(format!("{stem}.{}", format.extension()))).to_string_lossy().into_owned()
}

#[tauri::command]
fn exists(path: String) -> bool {
    Path::new(&path).exists()
}

#[tauri::command]
fn start_job(app: AppHandle, state: State<AppState>, id: u32, spec: JobSpec) {
    jobs::start(&app, id, spec, state.conflict());
}

#[tauri::command]
fn cancel_job(state: State<AppState>, id: u32) {
    state.jobs.cancel(id);
}

/// Drags entries of the open archive out to Explorer (or any drop target): only they are
/// extracted, into a temporary folder, then the system drag starts with those files.
/// Ends when they are dropped or the drag is cancelled: "dropped" | "cancelled".
#[tauri::command]
async fn drag_out(
    window: tauri::WebviewWindow,
    archive: String,
    password: Option<String>,
    items: Vec<String>,
    base: String,
) -> Result<String, String> {
    let paths = tauri::async_runtime::spawn_blocking(move || {
        jobs::extract_for_drag(Path::new(&archive), password.as_deref(), &items, &base).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;

    // DoDragDrop runs its own loop on the UI thread until the drop
    let (tx, rx) = std::sync::mpsc::channel::<Result<String, String>>();
    let target = window.clone();
    window
        .run_on_main_thread(move || {
            let done = tx.clone();
            let icon = drag::Image::Raw(include_bytes!("../icons/64x64.png").to_vec());
            let result = drag::start_drag(
                &target,
                drag::DragItem::Files(paths),
                icon,
                move |result, _| {
                    let _ = done.send(Ok(match result {
                        drag::DragResult::Dropped => "dropped".into(),
                        drag::DragResult::Cancel => "cancelled".into(),
                    }));
                },
                drag::Options::default(),
            );
            if let Err(e) = result {
                let _ = tx.send(Err(e.to_string()));
            }
        })
        .map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || rx.recv().unwrap_or_else(|_| Ok("cancelled".into())))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
fn pause_job(state: State<AppState>, id: u32, paused: bool) {
    state.jobs.pause(id, paused);
}

#[tauri::command]
fn is_busy(state: State<AppState>) -> bool {
    state.jobs.is_busy()
}

/// Shows a folder, or a file selected in its folder, in Explorer.
#[tauri::command]
fn reveal(app: AppHandle, path: String) -> Result<(), String> {
    let p = PathBuf::from(&path);
    if p.is_dir() {
        app.opener().open_path(path, None::<&str>).map_err(|e| e.to_string())
    } else {
        app.opener().reveal_item_in_dir(p).map_err(|e| e.to_string())
    }
}

#[tauri::command]
fn set_config(state: State<AppState>, config: Config) -> Result<(), String> {
    let (menu_changed, language_changed) = {
        let mut current = state.config.lock().unwrap();
        let changes = (current.context_menu != config.context_menu, current.language != config.language);
        let (width, height) = (current.window_width, current.window_height);
        *current = Config { window_width: width, window_height: height, ..config.clone() };
        current.save();
        changes
    };
    if menu_changed || (language_changed && config.context_menu) {
        let result = if config.context_menu { shell::register(&config.language) } else { shell::unregister() };
        result.map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// About > PayPal: the donation page in the default browser. The address is fixed here,
/// so the page cannot open arbitrary URLs.
#[tauri::command]
fn open_donate() -> Result<(), String> {
    open_in_browser(DONATE_URL)
}

const DONATE_URL: &str = "https://www.paypal.com/donate/?business=sergiisidko%40gmail.com&currency_code=USD";

/// Opens a project's web page from About > Licenses. Only http(s) addresses are accepted,
/// so the page cannot start programs or open local files.
#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    let web = url.starts_with("https://") || url.starts_with("http://");
    if !web || url.chars().any(|c| c.is_whitespace() || c == '"') {
        return Err("Not a web address".into());
    }
    open_in_browser(&url)
}

fn open_in_browser(url: &str) -> Result<(), String> {
    // The URL protocol handler always hands the address to the default browser
    // (`explorer <url>` can open a folder instead because of `&` and `?`)
    #[cfg(windows)]
    let mut command = std::process::Command::new("rundll32");
    #[cfg(windows)]
    command.args(["url.dll,FileProtocolHandler", url]);
    #[cfg(not(windows))]
    let mut command = std::process::Command::new("xdg-open");
    #[cfg(not(windows))]
    command.arg(url);
    command.spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[tauri::command]
fn quit(app: AppHandle) {
    quit_app(&app);
}

// ------------------------------------------------------------------ app logic

/// The main window, created when it was closed or never opened (Explorer menu starts).
fn show_window(app: &AppHandle) {
    if app.get_webview_window("main").is_none() {
        let config = app.state::<AppState>().config.lock().unwrap().clone();
        if let Err(e) = create_window(app, &config) {
            log(&format!("Failed to create the window: {e}"));
            return;
        }
    }
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// A small window with the progress of one task. It sizes and shows itself once its content is ready.
fn create_task_window(app: &AppHandle, task: Task) -> tauri::Result<()> {
    let state = app.state::<AppState>();
    let id = state.next_task.fetch_add(1, Ordering::SeqCst) + 1;
    state.tasks.lock().unwrap().insert(id, task);
    let builder = tauri::WebviewWindowBuilder::new(app, format!("task-{id}"), tauri::WebviewUrl::default())
        .title("Arc")
        .inner_size(440.0, 190.0)
        .resizable(false)
        .maximizable(false)
        .center()
        .decorations(false)
        .general_autofill_enabled(false)
        .visible(false);
    #[cfg(windows)]
    let builder = builder
        .transparent(true)
        .effects(tauri::window::EffectsBuilder::new().effect(tauri::window::Effect::Mica).build());
    builder.build()?;
    Ok(())
}

/// Explorer starts Arc once per selected file. The requests that arrive within a short
/// pause are merged, so ten selected archives become one batch of ten.
fn queue_request(app: &AppHandle, req: OpenRequest) {
    let state = app.state::<AppState>();
    {
        let mut batch = state.batch.lock().unwrap();
        batch.requests.push(req);
        batch.deadline = Some(Instant::now() + Duration::from_millis(450));
    }
    if state.batching.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        loop {
            let deadline = state.batch.lock().unwrap().deadline;
            match deadline {
                Some(d) if d > Instant::now() => std::thread::sleep(d - Instant::now()),
                _ => break,
            }
        }
        let requests = {
            let mut batch = state.batch.lock().unwrap();
            batch.deadline = None;
            state.batching.store(false, Ordering::SeqCst);
            std::mem::take(&mut batch.requests)
        };
        let mut merged: Vec<OpenRequest> = Vec::new();
        for req in requests {
            match merged.iter_mut().find(|m| m.action == req.action) {
                Some(m) => {
                    for p in req.paths {
                        if !m.paths.contains(&p) {
                            m.paths.push(p);
                        }
                    }
                }
                None => merged.push(req),
            }
        }
        for req in merged {
            if req.action == "open" {
                // Queued, not sent: the UI may not be loaded yet. It takes the queue on start and on this signal.
                state.pending_open.lock().unwrap().push(req);
                show_window(&app);
                let _ = app.emit_to("main", "open-pending", ());
            } else {
                // Explorer menu: just a small progress window, not the main one
                let task = Task { action: req.action, paths: req.paths, specs: None };
                if let Err(e) = create_task_window(&app, task) {
                    log(&format!("Failed to create a task window: {e}"));
                }
            }
        }
    });
}

fn quit_app(app: &AppHandle) {
    let state = app.state::<AppState>();
    if state.quitting.swap(true, Ordering::SeqCst) {
        return;
    }
    // Cancelled jobs remove their partial output; give them a moment
    state.jobs.cancel_all();
    let start = Instant::now();
    while state.jobs.is_busy() && start.elapsed() < Duration::from_secs(3) {
        std::thread::sleep(Duration::from_millis(30));
    }
    remember_window_size(app);
    app.exit(0);
}

fn remember_window_size(app: &AppHandle) {
    let Some(w) = app.get_webview_window("main") else { return };
    if w.is_minimized().unwrap_or(false) || w.is_maximized().unwrap_or(false) || !w.is_visible().unwrap_or(false) {
        return;
    }
    let (Ok(size), Ok(scale)) = (w.inner_size(), w.scale_factor()) else { return };
    let logical = size.to_logical::<f64>(scale);
    let state = app.state::<AppState>();
    let mut config = state.config.lock().unwrap();
    config.window_width = Some(logical.width.round());
    config.window_height = Some(logical.height.round());
    config.save();
}

const MIN_WIDTH: f64 = 480.0;
const MIN_HEIGHT: f64 = 360.0;

/// The window is created here rather than in tauri.conf.json so it starts at the remembered size.
fn create_window(app: &AppHandle, config: &Config) -> tauri::Result<()> {
    let width = config.window_width.unwrap_or(860.0).max(MIN_WIDTH);
    let height = config.window_height.unwrap_or(560.0).max(MIN_HEIGHT);
    let builder = tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::default())
        .title("Arc")
        .inner_size(width, height)
        .min_inner_size(MIN_WIDTH, MIN_HEIGHT)
        .center()
        .decorations(false)
        // No browser suggestions popping up over the password fields
        .general_autofill_enabled(false)
        .visible(false);
    // Windows 11: Mica backdrop behind the semi-transparent UI
    #[cfg(windows)]
    let builder = builder
        .transparent(true)
        .effects(tauri::window::EffectsBuilder::new().effect(tauri::window::Effect::Mica).build());
    builder.build()?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Must be first: a second launch hands its arguments to the running instance and exits
        .plugin(tauri_plugin_single_instance::init(|app, argv, cwd| match parse_args(&argv, Path::new(&cwd)) {
            Some(req) => queue_request(app, req),
            None => show_window(app),
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .setup(|app| {
            let handle = app.handle().clone();
            let state = app.state::<AppState>();
            let config = Config::load();
            *state.config.lock().unwrap() = config.clone();

            // Files opened from archives in an earlier session
            let _ = std::fs::remove_dir_all(config::temp_dir());

            // The exe path changes with updates and reinstalls: keep the registry current
            if config.context_menu
                && let Err(e) = shell::register(&config.language)
            {
                log(&format!("Context menu registration failed: {e}"));
            }

            // Started from the Explorer menu: no main window, only the task's progress window
            let cwd = std::env::current_dir().unwrap_or_default();
            let args: Vec<String> = std::env::args().collect();
            match parse_args(&args, &cwd) {
                Some(req) => {
                    if req.action == "open" {
                        create_window(&handle, &config)?;
                    }
                    queue_request(&handle, req);
                }
                None => {
                    create_window(&handle, &config)?;
                    show_window(&handle);
                }
            }

            let reporter = handle.clone();
            std::thread::spawn(move || jobs::report_loop(reporter));
            Ok(())
        })
        .on_window_event(|window, event| {
            let app = window.app_handle();
            if let WindowEvent::CloseRequested { api, .. } = event {
                if app.state::<AppState>().quitting.load(Ordering::SeqCst) {
                    return;
                }
                if window.label() == "main" {
                    // Task windows keep running; Arc exits with its last window
                    remember_window_size(app);
                } else {
                    // A task window cancels its job first and then closes itself
                    api.prevent_close();
                    let _ = app.emit_to(window.label(), "close-requested", ());
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_state,
            take_pending_open,
            list_archive,
            path_info,
            suggest_archive,
            exists,
            take_task,
            open_task,
            start_job,
            cancel_job,
            pause_job,
            drag_out,
            is_busy,
            reveal,
            set_config,
            open_donate,
            open_url,
            quit,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
