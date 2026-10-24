<!-- Contents of the open archive: path bar, sortable columns, a virtualized list of the current folder. -->
<script lang="ts">
  import {
    ArrowUp,
    ChevronDown,
    ChevronRight,
    ChevronUp,
    ExternalLink,
    File,
    FileArchive,
    FileAudio,
    FileCode,
    FileImage,
    FileSpreadsheet,
    FileText,
    FileVideo,
    Folder,
    FolderInput,
    FolderOpen,
    FolderOutput,
    Lock,
    Search,
    SquareCheckBig,
  } from "@lucide/svelte";
  import { tick } from "svelte";
  import * as actions from "../actions";
  import { formatDate, formatSize } from "../format";
  import { t } from "../i18n.svelte";
  import { selection, setCwd, view } from "../store.svelte";
  import { sortNodes, type Node, type SortKey } from "../tree";
  import Menu, { type MenuItem } from "./Menu.svelte";

  const ROW = 30;

  const archive = $derived(view.archive!);
  const showPacked = $derived(archive.listing.entries.some((e) => e.packed > 0));

  let filter = $state("");
  let list: HTMLDivElement;
  let scrollTop = $state(0);
  let height = $state(400);
  /** Anchor of shift-click ranges */
  let anchor: string | null = null;
  let menu = $state<{ items: MenuItem[]; x: number; y: number } | null>(null);

  const nodes = $derived.by(() => {
    const all = archive.tree.children.get(view.cwd) ?? [];
    const query = filter.trim().toLowerCase();
    const shown = query ? all.filter((n) => n.name.toLowerCase().includes(query)) : all;
    return sortNodes(shown, view.sort, view.desc);
  });

  // A new folder starts at the top
  $effect(() => {
    view.cwd;
    filter = "";
    if (list) list.scrollTop = 0;
  });

  const first = $derived(Math.max(0, Math.floor(scrollTop / ROW) - 8));
  const last = $derived(Math.min(nodes.length, Math.ceil((scrollTop + height) / ROW) + 8));
  const visible = $derived(nodes.slice(first, last));

  const crumbs = $derived.by(() => {
    const parts = view.cwd ? view.cwd.split("/") : [];
    return parts.map((name, i) => ({ name, path: parts.slice(0, i + 1).join("/") }));
  });

  function sortBy(key: SortKey) {
    if (view.sort === key) view.desc = !view.desc;
    else {
      view.sort = key;
      view.desc = key !== "name";
    }
  }

  function select(node: Node, e: MouseEvent | KeyboardEvent) {
    if (e.shiftKey && anchor) {
      const a = nodes.findIndex((n) => n.path === anchor);
      const b = nodes.findIndex((n) => n.path === node.path);
      if (!e.ctrlKey) selection.clear();
      for (let i = Math.min(a, b); i <= Math.max(a, b); i++) selection.add(nodes[i].path);
      return;
    }
    if (e.ctrlKey || e.metaKey) {
      if (selection.has(node.path)) selection.delete(node.path);
      else selection.add(node.path);
    } else {
      selection.clear();
      selection.add(node.path);
    }
    anchor = node.path;
  }

  function oncontextmenu(e: MouseEvent, node: Node | null) {
    e.preventDefault();
    if (node && !selection.has(node.path)) {
      selection.clear();
      selection.add(node.path);
      anchor = node.path;
    }
    const single = node && selection.size === 1 ? node : null;
    const only = selection.size > 0;
    const items: MenuItem[] = [];
    if (single) {
      items.push({ label: single.isDir ? t("open_folder") : t("open_file"), icon: single.isDir ? FolderOpen : ExternalLink, shortcut: "Enter", action: () => actions.openNode(single) });
      items.push({ separator: true });
    }
    items.push(
      { label: only ? t("extract_selected_here") : t("extract_here"), icon: FolderInput, action: () => actions.extractCurrent("here", only) },
      { label: t("extract_to_folder"), icon: FolderOutput, action: () => actions.extractCurrent("folder", only) },
      { label: t("extract_choose"), icon: FolderOpen, action: () => actions.extractCurrent("choose", only) },
      { separator: true },
      { label: t("select_all"), icon: SquareCheckBig, shortcut: "Ctrl+A", action: selectAll },
    );
    menu = { items, x: e.clientX, y: e.clientY };
  }

  function selectAll() {
    for (const n of nodes) selection.add(n.path);
  }

  /** Keyboard: arrows move, Enter opens, Backspace goes up */
  async function onkeydown(e: KeyboardEvent) {
    if (e.target instanceof HTMLInputElement) {
      if (e.key === "Escape") filter = "";
      if (e.key === "ArrowDown" && nodes.length) {
        e.preventDefault();
        list.focus();
        move(0, e);
      }
      return;
    }
    const current = nodes.findIndex((n) => n.path === anchor);
    if (e.key === "ArrowDown") move(current + 1, e);
    else if (e.key === "ArrowUp") move(Math.max(0, current - 1), e);
    else if (e.key === "Home") move(0, e);
    else if (e.key === "End") move(nodes.length - 1, e);
    else if (e.key === "PageDown") move(Math.min(nodes.length - 1, current + Math.floor(height / ROW)), e);
    else if (e.key === "PageUp") move(Math.max(0, current - Math.floor(height / ROW)), e);
    else if (e.key === "Enter" && current >= 0) actions.openNode(nodes[current]);
    else if (e.key === "Backspace" || (e.altKey && e.key === "ArrowUp")) actions.goUp();
    else if (e.key === "a" && e.ctrlKey) selectAll();
    else if (e.key === "Escape" && selection.size) selection.clear();
    else return;
    e.preventDefault();
    e.stopPropagation();
  }

  async function move(index: number, e: KeyboardEvent) {
    const node = nodes[Math.min(index, nodes.length - 1)];
    if (!node) return;
    if (e.shiftKey) select(node, e);
    else {
      selection.clear();
      selection.add(node.path);
      anchor = node.path;
    }
    await tick();
    const top = nodes.indexOf(node) * ROW;
    if (top < list.scrollTop) list.scrollTop = top;
    else if (top + ROW > list.scrollTop + height) list.scrollTop = top + ROW - height;
  }

  const ICONS: [RegExp, typeof File][] = [
    [/\.(zip|7z|rar|gz|tgz|xz|txz|tar|bz2|zst|iso|cab)$/i, FileArchive],
    [/\.(png|jpe?g|gif|webp|bmp|svg|ico|tiff?|heic|avif|raw|psd)$/i, FileImage],
    [/\.(mp4|mkv|avi|mov|wmv|webm|flv|m4v)$/i, FileVideo],
    [/\.(mp3|wav|flac|ogg|m4a|aac|wma|opus)$/i, FileAudio],
    [/\.(xlsx?|csv|ods)$/i, FileSpreadsheet],
    [/\.(txt|md|docx?|odt|rtf|pdf|log)$/i, FileText],
    [/\.(js|ts|rs|py|c|cpp|h|cs|java|go|json|xml|html?|css|sh|ps1|bat|toml|ya?ml|svelte)$/i, FileCode],
  ];
  const iconFor = (name: string) => ICONS.find(([re]) => re.test(name))?.[1] ?? File;
</script>

<div class="view" role="presentation" {onkeydown}>
  <div class="pathbar">
    <button class="up" title="{t('up')} (Backspace)" disabled={!view.cwd} onclick={actions.goUp}>
      <ArrowUp size={16} />
    </button>
    <nav class="crumbs">
      <button class="crumb" class:current={!view.cwd} onclick={() => setCwd("")}>
        <FileArchive size={14} />
        <span class="ellipsis">{archive.name}</span>
      </button>
      {#each crumbs as crumb, i (crumb.path)}
        <ChevronRight size={14} class="sep" />
        <button class="crumb" class:current={i === crumbs.length - 1} onclick={() => setCwd(crumb.path)}>
          <span class="ellipsis">{crumb.name}</span>
        </button>
      {/each}
    </nav>
    <label class="search">
      <Search size={14} />
      <input type="text" placeholder={t("filter")} bind:value={filter} spellcheck="false" />
    </label>
  </div>

  <div class="head" class:packed={showPacked}>
    {#each [["name", t("col_name")], ["size", t("col_size")], ...(showPacked ? [["packed", t("col_packed")]] : []), ["modified", t("col_modified")]] as [key, label] (key)}
      <button class="col col-{key}" onclick={() => sortBy(key as SortKey)}>
        <span>{label}</span>
        {#if view.sort === key}
          {#if view.desc}<ChevronDown size={13} />{:else}<ChevronUp size={13} />{/if}
        {/if}
      </button>
    {/each}
  </div>

  <div
    class="list"
    role="listbox"
    aria-multiselectable="true"
    tabindex="0"
    bind:this={list}
    bind:clientHeight={height}
    onscroll={() => (scrollTop = list.scrollTop)}
    onpointerdown={(e) => e.target === e.currentTarget && selection.clear()}
    oncontextmenu={(e) => e.target === e.currentTarget && oncontextmenu(e, null)}
  >
    {#if nodes.length === 0}
      <div class="empty">{filter ? t("nothing_found") : t("empty_folder")}</div>
    {/if}
    <div class="spacer" style:height="{nodes.length * ROW}px">
      {#each visible as node, i (node.path)}
        {@const Icon = node.isDir ? Folder : iconFor(node.name)}
        <div
          class="row"
          class:packed={showPacked}
          class:selected={selection.has(node.path)}
          style:top="{(first + i) * ROW}px"
          role="option"
          tabindex="-1"
          aria-selected={selection.has(node.path)}
          onpointerdown={(e) => e.button === 0 && select(node, e)}
          ondblclick={() => actions.openNode(node)}
          oncontextmenu={(e) => oncontextmenu(e, node)}
        >
          <span class="cell name">
            <span class="icon" class:dir={node.isDir}><Icon size={16} /></span>
            <span class="ellipsis" title={node.name}>{node.name}</span>
            {#if node.encrypted}<Lock size={12} class="lock" />{/if}
          </span>
          <span class="cell num" title={node.isDir ? t("files_count", node.files) : undefined}>{formatSize(node.size)}</span>
          {#if showPacked}<span class="cell num muted">{node.packed ? formatSize(node.packed) : ""}</span>{/if}
          <span class="cell date muted">{formatDate(node.modified)}</span>
        </div>
      {/each}
    </div>
  </div>
</div>

{#if menu}
  <Menu items={menu.items} x={menu.x} y={menu.y} onclose={() => (menu = null)} />
{/if}

<style>
  .view {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }
  .pathbar {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 10px 8px;
  }
  .up {
    display: grid;
    place-items: center;
    flex-shrink: 0;
    width: 30px;
    height: 30px;
    border: none;
    border-radius: 8px;
    background: var(--surface-hover);
    color: var(--text-2);
  }
  .up:hover {
    background: var(--surface-active);
    color: var(--text);
  }
  .up:disabled {
    opacity: 0.4;
  }
  .crumbs {
    display: flex;
    align-items: center;
    gap: 1px;
    flex: 1;
    min-width: 0;
    height: 30px;
    padding: 0 4px;
    border-radius: 8px;
    background: var(--surface);
    border: 1px solid var(--border);
    overflow: hidden;
  }
  .crumbs :global(.sep) {
    flex-shrink: 0;
    color: var(--text-3);
  }
  .crumb {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    max-width: 220px;
    height: 24px;
    padding: 0 6px;
    border: none;
    border-radius: 5px;
    background: none;
    color: var(--text-2);
    font-size: 13px;
  }
  .crumb:hover {
    background: var(--surface-hover);
    color: var(--text);
  }
  .crumb.current {
    color: var(--text);
    font-weight: 600;
  }
  .search {
    display: flex;
    align-items: center;
    gap: 6px;
    flex: 0 1 200px;
    min-width: 110px;
    height: 30px;
    padding: 0 10px;
    border-radius: 8px;
    border: 1px solid var(--border);
    background: var(--surface);
    color: var(--text-3);
    transition: border-color 0.15s, box-shadow 0.15s;
  }
  .search:focus-within {
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--accent-soft);
  }
  .search input {
    flex: 1;
    min-width: 0;
    border: none;
    outline: none;
    background: none;
    font-size: 13px;
  }

  /* Columns: name | size | (packed) | modified */
  .head,
  .row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 96px 132px;
    align-items: center;
  }
  .head.packed,
  .row.packed {
    grid-template-columns: minmax(0, 1fr) 96px 96px 132px;
  }
  @media (max-width: 620px) {
    .head,
    .row,
    .head.packed,
    .row.packed {
      grid-template-columns: minmax(0, 1fr) 88px;
    }
    .col-packed,
    .col-modified,
    .row .cell:nth-child(n + 3) {
      display: none;
    }
  }
  .head {
    margin: 0 10px;
    padding: 0 6px;
    border-bottom: 1px solid var(--border);
  }
  .col {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    height: 28px;
    padding: 0 6px;
    border: none;
    background: none;
    color: var(--text-3);
    font-size: 12px;
    font-weight: 600;
    text-align: left;
  }
  .col:hover {
    color: var(--text);
  }
  .col-size,
  .col-packed {
    justify-content: flex-end;
  }
  .list {
    position: relative;
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 4px 10px 8px;
    outline: none;
  }
  .spacer {
    position: relative;
  }
  .row {
    position: absolute;
    left: 0;
    right: 0;
    height: 30px;
    padding: 0 6px;
    border-radius: 7px;
    font-size: 13px;
    cursor: default;
  }
  .row:hover {
    background: var(--surface-hover);
  }
  .row.selected {
    background: var(--accent-soft);
  }
  .row.selected:hover {
    background: var(--accent-soft-strong);
  }
  .list:focus-visible .row.selected {
    box-shadow: inset 0 0 0 1px var(--accent);
  }
  .cell {
    padding: 0 6px;
    min-width: 0;
    white-space: nowrap;
  }
  .name {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .icon {
    display: inline-flex;
    flex-shrink: 0;
    color: var(--text-3);
  }
  .icon.dir {
    color: var(--accent);
  }
  .name :global(.lock) {
    flex-shrink: 0;
    color: var(--text-3);
  }
  .num {
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .date {
    font-variant-numeric: tabular-nums;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .empty {
    padding: 40px 0;
    text-align: center;
    color: var(--text-3);
  }
</style>
