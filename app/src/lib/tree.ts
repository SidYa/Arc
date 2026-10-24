// Folder structure of an archive from its flat entry list. Many archives have no entries
// for folders, so the missing ones are made up; folder sizes are the sums of their contents.
import type { Entry } from "./api";

export interface Node {
  name: string;
  path: string;
  isDir: boolean;
  size: number;
  packed: number;
  modified: number | null;
  encrypted: boolean;
  /** Files inside, recursively (folders only) */
  files: number;
}

export interface Tree {
  /** Folder path ("" is the root) -> its children */
  children: Map<string, Node[]>;
  files: number;
  size: number;
}

export const parentOf = (path: string) => {
  const i = path.lastIndexOf("/");
  return i < 0 ? "" : path.slice(0, i);
};

export function buildTree(entries: Entry[]): Tree {
  const nodes = new Map<string, Node>();
  const children = new Map<string, Map<string, Node>>([["", new Map()]]);

  const dir = (path: string): Node | null => {
    if (!path) return null;
    let node = nodes.get(path);
    if (!node) {
      node = { name: path.slice(path.lastIndexOf("/") + 1), path, isDir: true, size: 0, packed: 0, modified: null, encrypted: false, files: 0 };
      nodes.set(path, node);
      children.set(path, new Map());
      dir(parentOf(path));
      children.get(parentOf(path))!.set(node.name, node);
    }
    return node;
  };

  for (const e of entries) {
    if (e.is_dir) {
      const node = dir(e.path)!;
      node.modified = e.modified ?? node.modified;
      continue;
    }
    // The same path twice (updated archives): the later entry wins
    const parent = parentOf(e.path);
    dir(parent);
    const node: Node = {
      name: e.path.slice(e.path.lastIndexOf("/") + 1),
      path: e.path,
      isDir: false,
      size: e.size,
      packed: e.packed,
      modified: e.modified,
      encrypted: e.encrypted,
      files: 0,
    };
    nodes.set(e.path, node);
    children.get(parent)!.set(node.name, node);
  }

  // Sums bottom-up
  const sum = (path: string): [number, number, number] => {
    let size = 0, packed = 0, files = 0;
    for (const child of children.get(path)?.values() ?? []) {
      if (child.isDir) {
        const [s, p, f] = sum(child.path);
        child.size = s;
        child.packed = p;
        child.files = f;
        child.encrypted = [...(children.get(child.path)?.values() ?? [])].some((c) => c.encrypted);
      }
      size += child.size;
      packed += child.packed;
      files += child.isDir ? child.files : 1;
    }
    return [size, packed, files];
  };
  const [size, , files] = sum("");

  return {
    children: new Map([...children].map(([k, v]) => [k, [...v.values()]])),
    files,
    size,
  };
}

export type SortKey = "name" | "size" | "packed" | "modified";

const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: "base" });

/** Folders first, then by the key */
export function sortNodes(nodes: Node[], key: SortKey, desc: boolean): Node[] {
  const sign = desc ? -1 : 1;
  return [...nodes].sort((a, b) => {
    if (a.isDir !== b.isDir) return a.isDir ? -1 : 1;
    let r = 0;
    if (key === "size") r = a.size - b.size;
    else if (key === "packed") r = a.packed - b.packed;
    else if (key === "modified") r = (a.modified ?? 0) - (b.modified ?? 0);
    if (r === 0) r = collator.compare(a.name, b.name);
    return r * sign;
  });
}
