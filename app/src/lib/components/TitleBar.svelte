<!-- Custom title bar: every action lives here; empty space drags the window. -->
<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import {
    Copy,
    Ellipsis,
    File,
    Folder,
    FolderInput,
    FolderOpen,
    FolderOutput,
    Info,
    Minus,
    PackageOpen,
    PackagePlus,
    Power,
    Settings,
    Square,
    X,
    XCircle,
  } from "@lucide/svelte";
  import { onMount } from "svelte";
  import * as actions from "../actions";
  import { t } from "../i18n.svelte";
  import { openDialog, selection, view } from "../store.svelte";
  import Menu, { type MenuItem } from "./Menu.svelte";

  const win = getCurrentWindow();
  let menu = $state<{ items: MenuItem[]; x: number; y: number; align: "left" | "right" } | null>(null);
  let maximized = $state(false);

  onMount(() => {
    const update = async () => (maximized = await win.isMaximized());
    update();
    const unlisten = win.onResized(update);
    return () => unlisten.then((f) => f());
  });

  function openMenu(e: MouseEvent, items: MenuItem[], align: "left" | "right" = "left") {
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    menu = { items, x: align === "left" ? rect.left : rect.right, y: rect.bottom + 4, align };
  }

  const createItems = (): MenuItem[] => [
    { label: t("add_files"), icon: File, action: actions.pickFilesToArchive },
    { label: t("add_folder"), icon: Folder, action: actions.pickFolderToArchive },
  ];

  const extractItems = (): MenuItem[] => {
    const only = selection.size > 0;
    return [
      { label: t("extract_here"), icon: FolderInput, shortcut: "Ctrl+E", action: () => actions.extractCurrent("here", only) },
      { label: t("extract_to_folder"), icon: FolderOutput, action: () => actions.extractCurrent("folder", only) },
      { label: t("extract_choose"), icon: FolderOpen, action: () => actions.extractCurrent("choose", only) },
    ];
  };

  const moreItems = (): MenuItem[] => [
    { label: t("close_archive"), icon: XCircle, shortcut: "Ctrl+W", disabled: !view.archive, action: actions.closeArchive },
    { separator: true },
    { label: t("preferences"), icon: Settings, shortcut: "Ctrl+,", action: () => openDialog("settings", {}) },
    { label: t("about"), icon: Info, action: () => openDialog("about", {}) },
    { separator: true },
    { label: t("quit"), icon: Power, shortcut: "Ctrl+Q", action: actions.quit },
  ];
</script>

<header class="bar" data-tauri-drag-region>
  <div class="brand" data-tauri-drag-region>
    <img src="/favicon.png" alt="" width="20" height="20" data-tauri-drag-region />
    <span class="title" data-tauri-drag-region>Arc</span>
    {#if view.archive}
      <span class="archive ellipsis" title={view.archive.path} data-tauri-drag-region>{view.archive.name}</span>
    {/if}
  </div>

  <div class="actions">
    <button class="tool labeled" title="{t('open_archive')} (Ctrl+O)" onclick={actions.pickArchive}>
      <FolderOpen size={16} /><span>{t("open")}</span>
    </button>
    <button class="tool labeled" title={t("create_archive")} onclick={(e) => openMenu(e, createItems())}>
      <PackagePlus size={16} /><span>{t("create")}</span>
    </button>
    <button
      class="tool labeled accent"
      title={selection.size ? t("extract_selected") : t("extract")}
      disabled={!view.archive}
      onclick={(e) => openMenu(e, extractItems())}
    >
      <PackageOpen size={16} /><span>{t("extract")}</span>
    </button>
    <button class="tool" title={t("more")} onclick={(e) => openMenu(e, moreItems(), "right")}>
      <Ellipsis size={18} />
    </button>
  </div>

  <div class="window">
    <button class="win" title={t("minimize")} aria-label={t("minimize")} onclick={() => win.minimize()}><Minus size={16} /></button>
    <button
      class="win"
      title={maximized ? t("restore") : t("maximize")}
      aria-label={maximized ? t("restore") : t("maximize")}
      onclick={() => win.toggleMaximize()}
    >
      {#if maximized}<Copy size={13} style="transform: scaleX(-1)" />{:else}<Square size={13} />{/if}
    </button>
    <button class="win close" title={t("close")} aria-label={t("close")} onclick={actions.closeWindow}><X size={16} /></button>
  </div>
</header>

{#if menu}
  <Menu items={menu.items} x={menu.x} y={menu.y} align={menu.align} onclose={() => (menu = null)} />
{/if}

<style>
  .bar {
    display: flex;
    align-items: center;
    height: 46px;
    padding-left: 12px;
    flex-shrink: 0;
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: 1;
    min-width: 0;
    height: 100%;
  }
  .brand img {
    flex-shrink: 0;
  }
  .title {
    flex-shrink: 0;
    font-weight: 600;
    font-size: 14px;
    letter-spacing: 0.2px;
  }
  .archive {
    min-width: 0;
    padding-left: 8px;
    border-left: 1px solid var(--border-strong);
    color: var(--text-2);
    font-size: 13px;
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 0 6px;
  }
  .tool {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    min-width: 32px;
    height: 32px;
    padding: 0 8px;
    border: none;
    border-radius: 8px;
    background: none;
    color: var(--text-2);
    font-size: 13px;
    transition: background 0.15s, color 0.15s, transform 0.1s;
  }
  .tool:hover {
    background: var(--surface-hover);
    color: var(--text);
  }
  .tool:active {
    transform: scale(0.95);
  }
  .tool:disabled {
    opacity: 0.35;
    pointer-events: none;
  }
  .tool.accent:not(:disabled) {
    color: var(--accent);
  }
  .tool.accent:not(:disabled):hover {
    background: var(--accent-soft);
  }
  /* Narrow window: icons only */
  @media (max-width: 700px) {
    .labeled span {
      display: none;
    }
    .archive {
      display: none;
    }
  }
  .window {
    display: flex;
    align-self: stretch;
  }
  .win {
    display: grid;
    place-items: center;
    width: 44px;
    border: none;
    background: none;
    color: var(--text-2);
    transition: background 0.12s, color 0.12s;
  }
  .win:hover {
    background: var(--surface-hover);
    color: var(--text);
  }
  .win.close:hover {
    background: #e81123;
    color: #fff;
  }
</style>
