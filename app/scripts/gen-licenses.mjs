// Builds static/licenses.json for About > Licenses (the same approach as Farba): every
// third-party component that ships inside Arc, with its licence and the licence text found
// in its sources.
//
//   bundled  C/C++ libraries compiled into the executable
//   rust     crates compiled into the app for the build target (CARGO_BUILD_TARGET, else this OS)
//   js       npm packages bundled into the UI (production deps + the Svelte runtime)
//
// Run: npm run licenses (also runs as part of `npm run build`)
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const appDir = join(dirname(fileURLToPath(import.meta.url)), "..");
const tauriDir = join(appDir, "src-tauri");
const out = join(appDir, "static", "licenses.json");

const texts = {};
function addText(text) {
  const norm = text.replace(/\r\n/g, "\n").trim();
  if (!norm) return null;
  const id = createHash("sha1").update(norm).digest("hex").slice(0, 12);
  texts[id] = norm;
  return id;
}

const LICENSE_FILE = /^(licen[cs]e|copying|notice|unlicense)([-._].*)?$/i;
function licenseTexts(dir) {
  if (!dir || !existsSync(dir)) return [];
  return readdirSync(dir)
    .filter((f) => LICENSE_FILE.test(f))
    .sort()
    .map((f) => {
      try {
        return addText(readFileSync(join(dir, f), "utf8"));
      } catch {
        return null;
      }
    })
    .filter(Boolean);
}

/** A licence file that may be missing */
function fileText(path) {
  return path && existsSync(path) ? [addText(readFileSync(path, "utf8"))] : [];
}

const packages = [];
/** Source folders of crates, for the C libraries they vendor */
const crateDirs = new Map();

// --- Rust ---------------------------------------------------------------
function rustCrates() {
  const home = process.env.CARGO_HOME ?? join(process.env.USERPROFILE ?? process.env.HOME ?? "", ".cargo");
  const cargo = existsSync(join(home, "bin", "cargo.exe")) ? join(home, "bin", "cargo.exe") : "cargo";
  const triple =
    process.env.CARGO_BUILD_TARGET ?? (process.platform === "win32" ? "x86_64-pc-windows-msvc" : "x86_64-unknown-linux-gnu");
  const raw = execFileSync(cargo, ["metadata", "--format-version", "1", "--filter-platform", triple], {
    cwd: tauriDir,
    encoding: "utf8",
    maxBuffer: 256 * 1024 * 1024,
  });
  const meta = JSON.parse(raw);
  const members = new Set(meta.workspace_members);
  // Walk the resolve graph from the app, normal deps only (build-time and dev-only crates are not shipped)
  const nodes = new Map(meta.resolve.nodes.map((n) => [n.id, n]));
  const seen = new Set();
  const stack = [...members];
  while (stack.length) {
    const id = stack.pop();
    if (seen.has(id)) continue;
    seen.add(id);
    for (const d of nodes.get(id)?.deps ?? []) {
      if (d.dep_kinds.some((k) => k.kind === null)) stack.push(d.pkg);
    }
  }
  for (const p of meta.packages) {
    crateDirs.set(p.name, dirname(p.manifest_path));
    if (!seen.has(p.id) || members.has(p.id)) continue;
    packages.push({
      kind: "rust",
      name: p.name,
      version: p.version,
      license: p.license ?? (p.license_file ? "see licence file" : "unknown"),
      repository: p.repository ?? p.homepage ?? `https://crates.io/crates/${p.name}`,
      texts: licenseTexts(dirname(p.manifest_path)),
    });
  }
}

// --- JavaScript ---------------------------------------------------------
function resolveDir(name, from) {
  let dir = from;
  while (true) {
    const cand = join(dir, "node_modules", name);
    if (existsSync(join(cand, "package.json"))) return cand;
    const up = dirname(dir);
    if (up === dir) return null;
    dir = up;
  }
}

function jsPackages() {
  const appPkg = JSON.parse(readFileSync(join(appDir, "package.json"), "utf8"));
  // Production deps + the Svelte runtime, which the compiler inlines into the bundle
  const roots = [...Object.keys(appPkg.dependencies ?? {}), "svelte"];
  const seen = new Set();
  const visit = (name, from, depth) => {
    const dir = resolveDir(name, from);
    if (!dir) return;
    const pkg = JSON.parse(readFileSync(join(dir, "package.json"), "utf8"));
    const key = `${pkg.name}@${pkg.version}`;
    if (seen.has(key)) return;
    seen.add(key);
    const repo = typeof pkg.repository === "string" ? pkg.repository : pkg.repository?.url;
    packages.push({
      kind: "js",
      name: pkg.name,
      version: pkg.version,
      license: typeof pkg.license === "string" ? pkg.license : (pkg.license?.type ?? "unknown"),
      repository: (repo ?? pkg.homepage ?? `https://www.npmjs.com/package/${pkg.name}`)
        .replace(/^git\+/, "")
        .replace(/\.git$/, ""),
      texts: licenseTexts(dir),
    });
    // Svelte's own deps are compiler-only; the runtime has none
    if (name === "svelte" || depth > 20) return;
    for (const dep of Object.keys(pkg.dependencies ?? {})) visit(dep, dir, depth + 1);
  };
  for (const r of roots) visit(r, appDir, 0);
}

try {
  rustCrates();
} catch (e) {
  console.warn("[licenses] cargo metadata failed:", e.message.split("\n")[0]);
}
jsPackages();

// --- C libraries compiled in (not discoverable from manifests) ---
const unrarDir = crateDirs.get("unrar_sys");
const xzDir = crateDirs.get("liblzma-sys");
const manual = [
  {
    kind: "bundled",
    name: "UnRAR",
    version: "",
    license: "UnRAR licence",
    repository: "https://www.rarlab.com/rar_add.htm",
    note: "RAR extraction. The UnRAR source may be used to extract RAR archives but not to create a RAR compatible archiver.",
    texts: fileText(unrarDir && join(unrarDir, "vendor", "unrar", "license.txt")),
  },
  {
    kind: "bundled",
    name: "XZ Utils (liblzma)",
    version: "",
    license: "0BSD",
    repository: "https://tukaani.org/xz/",
    note: "XZ compression and decompression (.tar.xz).",
    texts: [...fileText(xzDir && join(xzDir, "xz", "COPYING.0BSD"))],
  },
];

const order = { bundled: 0, rust: 1, js: 2 };
const all = [...manual, ...packages].sort((a, b) => order[a.kind] - order[b.kind] || a.name.localeCompare(b.name));
const missing = all.filter((p) => p.texts.length === 0).map((p) => `${p.name}@${p.version}`);
// No licence file in the sources: point at the canonical SPDX text
for (const p of all) {
  if (p.texts.length) continue;
  const ids = String(p.license)
    .split(/\s+(?:OR|AND|WITH)\s+|[()/]/)
    .map((x) => x.trim())
    .filter(Boolean);
  const links = ids.map((id) => `${id}: https://spdx.org/licenses/${id}.html`).join("\n");
  p.texts = [addText(`Licensed under ${p.license}.\n\n${links}`)];
}
writeFileSync(out, JSON.stringify({ generated: new Date().toISOString(), packages: all, texts }));
const count = (kind) => packages.filter((p) => p.kind === kind).length;
console.log(
  `[licenses] ${all.length} components (${count("rust")} rust, ${count("js")} js), ${Object.keys(texts).length} distinct texts -> ${out}`,
);
if (missing.length)
  console.log(`[licenses] no licence file found for ${missing.length}: ${missing.slice(0, 12).join(", ")}${missing.length > 12 ? ", …" : ""}`);
