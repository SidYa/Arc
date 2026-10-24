import { SvelteSet } from "svelte/reactivity";
import type { Config, Listing } from "./api";
import type { SortKey, Tree } from "./tree";

export const app = $state({
  config: null as Config | null,
  platform: "windows" as "windows" | "linux" | "macos",
  version: "",
  systemAccent: null as string | null,
  /** Toast-like message at the bottom of the window */
  notice: null as { text: string; kind: "error" | "info" } | null,
  /** Archive being read */
  loading: null as string | null,
  /** File being extracted from the archive to open it */
  opening: null as string | null,
});

// ---------------------------------------------------------------- open archive

export interface OpenArchive {
  path: string;
  name: string;
  password: string | null;
  listing: Listing;
  tree: Tree;
}

export const view = $state({
  archive: null as OpenArchive | null,
  /** Folder inside the archive, "" is the root */
  cwd: "",
  sort: "name" as SortKey,
  desc: false,
});

/** Paths of the selected entries in the current folder */
export const selection = new SvelteSet<string>();

export function setCwd(path: string) {
  view.cwd = path;
  selection.clear();
}

// ---------------------------------------------------------------- notices

let noticeTimer: ReturnType<typeof setTimeout> | undefined;
export function notify(text: string, kind: "error" | "info" = "info") {
  app.notice = { text, kind };
  clearTimeout(noticeTimer);
  noticeTimer = setTimeout(() => (app.notice = null), kind === "error" ? 7000 : 3500);
}

// ---------------------------------------------------------------- dialogs

/** Dialog kind -> [props, result] */
interface DialogTypes {
  confirm: [{ title: string; body: string; action: string; danger?: boolean }, boolean];
  message: [{ title: string; body: string }, void];
  password: [{ archive: string; wrong: boolean }, string | null];
  settings: [{}, void];
  about: [{}, void];
}

export type DialogKind = keyof DialogTypes;

export interface OpenDialog<K extends DialogKind = DialogKind> {
  id: number;
  kind: K;
  props: DialogTypes[K][0];
  resolve: (result: DialogTypes[K][1]) => void;
}

let nextDialogId = 1;
/** Dialogs wait in a queue; only the first one is shown. */
export const dialogs = $state<OpenDialog[]>([]);

export function openDialog<K extends DialogKind>(kind: K, props: DialogTypes[K][0]): Promise<DialogTypes[K][1]> {
  return new Promise((resolve) => {
    const id = nextDialogId++;
    dialogs.push({
      id,
      kind,
      props,
      resolve: (result: DialogTypes[K][1]) => {
        const index = dialogs.findIndex((d) => d.id === id);
        if (index >= 0) dialogs.splice(index, 1);
        resolve(result);
      },
    } as unknown as OpenDialog);
  });
}
