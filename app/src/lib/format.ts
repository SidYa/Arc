const UNITS = ["B", "KB", "MB", "GB", "TB"];

export function formatSize(bytes: number): string {
  let size = bytes;
  for (const unit of UNITS) {
    if (size < 1024) return unit === "B" ? `${Math.round(size)} ${unit}` : `${size.toFixed(1)} ${unit}`;
    size /= 1024;
  }
  return `${size.toFixed(1)} PB`;
}

export function formatSpeed(bytesPerSec: number): string {
  return `${formatSize(bytesPerSec)}/s`;
}

export function formatEta(seconds: number): string {
  if (!isFinite(seconds) || seconds <= 0) return "";
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = Math.floor(seconds % 60);
  if (h > 0) return `${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
  return `${m}:${String(s).padStart(2, "0")}`;
}

const dateFormat = new Intl.DateTimeFormat(undefined, { dateStyle: "short", timeStyle: "short" });

export function formatDate(unix: number | null): string {
  return unix == null ? "" : dateFormat.format(new Date(unix * 1000));
}

/** Last component of a Windows or Unix path */
export function baseName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

export function dirName(path: string): string {
  const i = Math.max(path.lastIndexOf("\\"), path.lastIndexOf("/"));
  return i > 0 ? path.slice(0, i) : path;
}

/** "archive.tar.gz" -> "tar.gz" */
export function extensionOf(path: string): string {
  const name = baseName(path).toLowerCase();
  for (const ext of ["tar.gz", "tar.xz"]) if (name.endsWith("." + ext)) return ext;
  const dot = name.lastIndexOf(".");
  return dot > 0 ? name.slice(dot + 1) : "";
}

/** Replaces the archive extension: "c:\a\b.zip" + "7z" -> "c:\a\b.7z" */
export function withExtension(path: string, ext: string): string {
  const name = baseName(path);
  const old = extensionOf(path);
  const stem = old && name.toLowerCase().endsWith("." + old) ? name.slice(0, name.length - old.length - 1) : name;
  return path.slice(0, path.length - name.length) + stem + "." + ext;
}
