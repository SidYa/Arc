// User actions of the main window: title bar, keyboard, drag & drop, "Open with".
// Archiving and extraction run in small progress windows of their own (TaskWindow.svelte).
import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api, type JobSpec, type OpenRequest } from "./api";
import { baseName, dirName } from "./format";
import { t } from "./i18n.svelte";
import { errorText, isPasswordError, newId, startJob, watchJob } from "./jobs";
import { app, notify, openDialog, selection, setCwd, view } from "./store.svelte";
import { buildTree, countFiles, type Node } from "./tree";

const ARCHIVE_EXTENSIONS = ["zip", "7z", "rar", "tar", "gz", "tgz", "xz", "txz"];

// ---------------------------------------------------------------- browsing

export async function openArchive(path: string): Promise<boolean> {
  let password: string | null = null;
  app.loading = path;
  try {
    for (;;) {
      try {
        const listing = await api.listArchive(path, password);
        view.archive = { path, name: baseName(path), password, listing, tree: buildTree(listing.entries) };
        setCwd("");
        return true;
      } catch (e) {
        const code = String(e);
        if (!isPasswordError(code)) throw e;
        // The file names themselves are encrypted
        app.loading = null;
        password = await openDialog("password", { archive: baseName(path), wrong: code === "wrong_password" });
        if (password == null) return false;
        app.loading = path;
      }
    }
  } catch (e) {
    notify(errorText(e), "error");
    return false;
  } finally {
    app.loading = null;
  }
}

export function closeArchive() {
  view.archive = null;
  selection.clear();
}

export async function pickArchive() {
  const path = await open({
    multiple: false,
    filters: [
      { name: t("archives"), extensions: ARCHIVE_EXTENSIONS },
      { name: t("all_files"), extensions: ["*"] },
    ],
  });
  if (typeof path === "string") await openArchive(path);
}

/** Double click: into a folder, or open the file with its program (only it is extracted) */
export async function openNode(node: Node) {
  const archive = view.archive;
  if (!archive) return;
  if (node.isDir) {
    setCwd(node.path);
    return;
  }
  let password = archive.password;
  app.opening = node.name;
  try {
    for (;;) {
      const expected_files = countFiles(archive.listing.entries, [node.path]);
      const spec: JobSpec = { kind: "open", archive: archive.path, password, item: node.path, expected_files };
      const job = await startJob(spec).finished;
      if (job.state === "done") {
        // Remember a password that worked
        if (password && view.archive?.path === archive.path) view.archive.password = password;
        return;
      }
      if (job.state !== "error") return;
      if (!isPasswordError(job.error)) {
        notify(errorText(job.error), "error");
        return;
      }
      app.opening = null;
      password = await openDialog("password", { archive: archive.name, wrong: job.error === "wrong_password" });
      if (password == null) return;
      app.opening = node.name;
    }
  } finally {
    app.opening = null;
  }
}

/** Drag the selected entries to Explorer. The drag starts at once with placeholders; only on the
 *  drop are the entries extracted (into a temporary folder), with the progress in the status bar.
 *  Called while the mouse button is still held. */
export async function dragOut() {
  const archive = view.archive;
  if (!archive || app.draggingOut || !selection.size) return;
  const nodes = (archive.tree.children.get(view.cwd) ?? []).filter((n) => selection.has(n.path));
  if (!nodes.length) return;
  // Encrypted entries need the password before the drag: the button is released by then
  if (!archive.password && nodes.some((n) => n.encrypted)) {
    await askDragPassword(archive.path, archive.name, false);
    return;
  }
  const items = nodes.map((n) => n.path);
  const id = newId();
  const watch = watchJob(id, (job) => (app.dragJob = job));
  app.draggingOut = true;
  try {
    await api.dragOut(id, archive.path, archive.password, items, nodes.map((n) => n.isDir), view.cwd, countFiles(archive.listing.entries, items));
  } catch (e) {
    const code = String(e);
    if (isPasswordError(code)) await askDragPassword(archive.path, archive.name, code === "wrong_password");
    else if (code !== "cancelled") notify(errorText(e), "error");
  } finally {
    watch.stop();
    app.dragJob = null;
    app.draggingOut = false;
  }
}

async function askDragPassword(path: string, name: string, wrong: boolean) {
  const password = await openDialog("password", { archive: name, wrong });
  if (password != null && view.archive?.path === path) {
    view.archive.password = password;
    notify(t("drag_again"));
  }
}

export function goUp() {
  if (!view.cwd) return;
  const i = view.cwd.lastIndexOf("/");
  setCwd(i < 0 ? "" : view.cwd.slice(0, i));
}

// ---------------------------------------------------------------- create & extract

export async function createArchive(sources: string[], password = false) {
  if (sources.length) await api.openTask({ action: password ? "add_password" : "add", paths: sources });
}

export async function pickFilesToArchive() {
  const files = await open({ multiple: true });
  if (Array.isArray(files) && files.length) await createArchive(files);
}

export async function pickFolderToArchive() {
  const folder = await open({ directory: true });
  if (typeof folder === "string") await createArchive([folder]);
}

export type ExtractTarget = "here" | "folder" | "choose";

/** The open archive, or only the selected entries of the current folder */
export async function extractCurrent(target: ExtractTarget, onlySelected = false) {
  const archive = view.archive;
  if (!archive) return;
  const items = onlySelected && selection.size ? [...selection] : null;
  let dest = "";
  if (target === "choose") {
    const dir = await open({ directory: true, defaultPath: dirName(archive.path) });
    if (typeof dir !== "string") return;
    dest = dir;
  }
  const spec: JobSpec = {
    kind: "extract",
    archive: archive.path,
    mode: target === "choose" ? "to" : target,
    dest,
    password: archive.password,
    items,
    base: items ? view.cwd : "",
    expected_files: items ? countFiles(archive.listing.entries, items) : null,
  };
  await api.openTask({ action: "jobs", specs: [spec] });
}

/** Several dropped archives: each into its own folder */
export async function extractArchives(paths: string[]) {
  await api.openTask({ action: "extract_folder", paths });
}

// ---------------------------------------------------------------- entry points

export async function handleOpenRequest(req: OpenRequest) {
  if (req.action === "open") await openPaths(req.paths);
  else await api.openTask({ action: req.action, paths: req.paths });
}

/** Dropped or opened paths: archives are opened, anything else is offered for archiving */
export async function openPaths(paths: string[]) {
  const infos = await api.pathInfo(paths);
  const archives = infos.filter((i) => i.format);
  if (archives.length && archives.length === infos.length) {
    if (archives.length === 1) {
      await openArchive(archives[0].path);
      return;
    }
    const ok = await openDialog("confirm", {
      title: t("extract_many_h", archives.length),
      body: t("extract_many_b"),
      action: t("extract_each"),
    });
    if (ok) await extractArchives(archives.map((a) => a.path));
    return;
  }
  await createArchive(paths);
}

/** The close button closes only this window: progress windows keep working */
export async function closeWindow() {
  await getCurrentWindow().close();
}

/** Quit: running operations are stopped only after asking */
export async function quit() {
  if (await api.isBusy()) {
    const ok = await openDialog("confirm", {
      title: t("quit_running_h"),
      body: t("quit_running_b"),
      action: t("quit_stop"),
      danger: true,
    });
    if (!ok) return;
  }
  await api.quit();
}
