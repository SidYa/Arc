// Typed wrappers over the Rust commands (src-tauri/src/lib.rs).
import { invoke } from "@tauri-apps/api/core";

export type Format = "zip" | "7z" | "tar.gz" | "tar.xz" | "tar" | "gz" | "xz" | "rar";

/** Formats Arc can create */
export const WRITABLE: Format[] = ["zip", "7z", "tar.gz", "tar.xz"];
export const supportsPassword = (f: Format) => f === "zip" || f === "7z";

export interface Entry {
  /** "/" separated, no trailing slash */
  path: string;
  is_dir: boolean;
  size: number;
  packed: number;
  /** Unix seconds */
  modified: number | null;
  encrypted: boolean;
}

export interface Listing {
  format: Format;
  entries: Entry[];
  encrypted: boolean;
  solid: boolean;
  size: number;
}

export interface Config {
  language: string;
  theme: "system" | "light" | "dark";
  accent: string;
  monochrome: boolean;
  context_menu: boolean;
  conflict: "rename" | "overwrite" | "skip";
  default_format: Format;
  default_level: number;
  open_folder_after: boolean;
}

export interface InitialState {
  config: Config;
  version: string;
  platform: "windows" | "linux" | "macos";
  system_accent: string | null;
}

export type Action = "open" | "add" | "add_password" | "extract_here" | "extract_folder";

/** Work for a small progress window */
export interface Task {
  action: Exclude<Action, "open"> | "jobs";
  paths?: string[];
  specs?: JobSpec[];
}

export interface OpenRequest {
  action: Action;
  paths: string[];
}

/** Which program opens one archive type */
export interface Association {
  ext: string;
  is_arc: boolean;
  app: string | null;
}

export interface PathInfo {
  path: string;
  name: string;
  is_dir: boolean;
  format: Format | null;
}

export interface CreateOptions {
  format: Format;
  level: number;
  password: string | null;
  encrypt_names: boolean;
}

export type JobSpec =
  | { kind: "create"; sources: string[]; dest: string; options: CreateOptions }
  | {
      kind: "extract";
      archive: string;
      dest?: string;
      mode: "to" | "here" | "folder";
      password: string | null;
      items?: string[] | null;
      base?: string;
    }
  | { kind: "open"; archive: string; password: string | null; item: string };

export interface JobProgress {
  id: number;
  done: number;
  total: number;
  current: string;
}

export interface JobFinished {
  id: number;
  ok: boolean;
  error: string | null;
  output: string | null;
}

export const api = {
  getState: () => invoke<InitialState>("get_state"),
  takePendingOpen: () => invoke<OpenRequest[]>("take_pending_open"),
  listArchive: (path: string, password: string | null) => invoke<Listing>("list_archive", { path, password }),
  pathInfo: (paths: string[]) => invoke<PathInfo[]>("path_info", { paths }),
  suggestArchive: (sources: string[], format: Format) => invoke<string>("suggest_archive", { sources, format }),
  exists: (path: string) => invoke<boolean>("exists", { path }),
  startJob: (id: number, spec: JobSpec) => invoke<void>("start_job", { id, spec }),
  cancelJob: (id: number) => invoke<void>("cancel_job", { id }),
  pauseJob: (id: number, paused: boolean) => invoke<void>("pause_job", { id, paused }),
  dragOut: (archive: string, password: string | null, items: string[], base: string) =>
    invoke<"dropped" | "cancelled">("drag_out", { archive, password, items, base }),
  isBusy: () => invoke<boolean>("is_busy"),
  takeTask: (id: number) => invoke<Task | null>("take_task", { id }),
  openTask: (task: Task) => invoke<void>("open_task", { task }),
  reveal: (path: string) => invoke<void>("reveal", { path }),
  setConfig: (config: Config) => invoke<void>("set_config", { config }),
  associations: () => invoke<Association[]>("associations"),
  associate: () => invoke<Association[]>("associate"),
  unassociate: () => invoke<Association[]>("unassociate"),
  openDefaultApps: () => invoke<void>("open_default_apps"),
  quit: () => invoke<void>("quit"),
};
