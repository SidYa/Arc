<script lang="ts">
  import "../app.css";
  import { onMount } from "svelte";
  import { fade, fly } from "svelte/transition";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { FileArchive, PackagePlus } from "@lucide/svelte";
  import { fitAccent } from "$lib/accent";
  import * as actions from "$lib/actions";
  import { api, type JobFinished, type JobProgress } from "$lib/api";
  import { setLanguage, t } from "$lib/i18n.svelte";
  import { app, dialogs, openDialog, view } from "$lib/store.svelte";
  import ArchiveView from "$lib/components/ArchiveView.svelte";
  import Dialogs from "$lib/components/Dialogs.svelte";
  import JobsPanel from "$lib/components/JobsPanel.svelte";
  import StatusBar from "$lib/components/StatusBar.svelte";
  import TitleBar from "$lib/components/TitleBar.svelte";
  import Welcome from "$lib/components/Welcome.svelte";

  /** What the dragged files would do: open an archive or make a new one */
  let dragging = $state<"open" | "create" | null>(null);
  let ready = $state(false);

  // Theme: explicit choice or follow the OS
  const systemDark = window.matchMedia("(prefers-color-scheme: dark)");
  let prefersDark = $state(systemDark.matches);
  systemDark.addEventListener("change", (e) => (prefersDark = e.matches));
  const dark = $derived((app.config?.theme ?? "system") === "system" ? prefersDark : app.config?.theme === "dark");
  $effect(() => {
    document.documentElement.dataset.theme = dark ? "dark" : "light";
  });
  // Accent: a preset name, the Windows accent or a custom #rrggbb; monochrome replaces it with gray
  $effect(() => {
    const root = document.documentElement;
    let accent = app.config?.monochrome ? "mono" : (app.config?.accent ?? "purple");
    if (accent === "system") accent = app.systemAccent ?? "purple";
    const custom = accent.startsWith("#");
    if (accent === "purple") delete root.dataset.accent;
    else root.dataset.accent = custom ? "custom" : accent;
    if (custom) {
      const fit = fitAccent(accent, dark);
      root.style.setProperty("--accent", fit.accent);
      root.style.setProperty("--accent-text", fit.text);
    } else {
      root.style.removeProperty("--accent");
      root.style.removeProperty("--accent-text");
    }
  });

  async function takePendingOpen() {
    for (const req of await api.takePendingOpen()) await actions.handleOpenRequest(req);
  }

  const ARCHIVE = /\.(zip|7z|rar|tar|gz|tgz|xz|txz)$/i;

  onMount(() => {
    const unlisten: Promise<() => void>[] = [];

    (async () => {
      const state = await api.getState();
      app.config = state.config;
      app.platform = state.platform;
      app.version = state.version;
      app.systemAccent = state.system_accent;
      app.taskMode = state.task_mode;
      document.documentElement.classList.add(`platform-${state.platform}`);
      setLanguage(state.config.language);
      ready = true;
      await takePendingOpen();
    })();

    unlisten.push(
      listen("open-pending", () => takePendingOpen()),
      listen<JobProgress[]>("job-progress", (e) => actions.onJobProgress(e.payload)),
      listen<JobFinished>("job-finished", (e) => actions.onJobFinished(e.payload)),
      listen("close-requested", () => actions.requestClose()),
      getCurrentWebview().onDragDropEvent(async (e) => {
        if (e.payload.type === "enter") {
          const paths = e.payload.paths;
          dragging = paths.length && paths.every((p) => ARCHIVE.test(p)) ? "open" : "create";
        } else if (e.payload.type === "leave") dragging = null;
        else if (e.payload.type === "drop") {
          dragging = null;
          if (!dialogs.length && e.payload.paths.length) await actions.openPaths(e.payload.paths);
        }
      }),
    );

    return () => unlisten.forEach((p) => p.then((f) => f()));
  });

  /** WebView2's own shortcuts (find bar, print, history, caret browsing…) don't belong in an app */
  function blockBrowserKeys(e: KeyboardEvent) {
    const ctrl = e.ctrlKey || e.metaKey;
    const key = e.key.toLowerCase();
    if (
      e.key === "F3" ||
      e.key === "F7" ||
      (ctrl && ["f", "g", "p", "j", "h", "u", "s", "r", "l"].includes(key)) ||
      (e.altKey && (e.key === "ArrowLeft" || e.key === "ArrowRight") && !(e.target instanceof HTMLInputElement))
    )
      e.preventDefault();
  }

  function onkeydown(e: KeyboardEvent) {
    // Shortcuts only when no dialog is open and the user is not typing
    if (dialogs.length || e.target instanceof HTMLTextAreaElement) return;
    const typing = e.target instanceof HTMLInputElement;
    const ctrl = e.ctrlKey || e.metaKey;
    const key = e.key.toLowerCase();
    if (ctrl && key === "o") actions.pickArchive();
    else if (ctrl && key === "n") actions.pickFilesToArchive();
    else if (ctrl && key === "e" && view.archive) actions.extractCurrent("here", true);
    else if (ctrl && key === "w" && view.archive) actions.closeArchive();
    else if (ctrl && key === "q") actions.requestClose();
    else if (ctrl && key === ",") openDialog("settings", {});
    else if (ctrl && key === "f" && view.archive) document.querySelector<HTMLInputElement>(".search input")?.focus();
    else if (!typing && (e.key === "F5" || (ctrl && key === "r"))) {
      /* no page reloads in an app */
    } else return;
    e.preventDefault();
  }
</script>

<svelte:window {onkeydown} onkeydowncapture={blockBrowserKeys} oncontextmenu={(e) => e.preventDefault()} />

<div class="app">
  <TitleBar />
  <main>
    {#if ready}
      {#if view.archive && !app.loading}
        <ArchiveView />
      {:else}
        <Welcome />
      {/if}
    {/if}
  </main>
  <JobsPanel />
  <StatusBar />
</div>

{#if dragging}
  <div class="drop" transition:fade={{ duration: 120 }}>
    <div class="drop-card" in:fly={{ y: 8, duration: 180 }}>
      {#if dragging === "open"}
        <FileArchive size={34} strokeWidth={1.6} />
        <div>{t("drop_open")}</div>
      {:else}
        <PackagePlus size={34} strokeWidth={1.6} />
        <div>{t("drop_create")}</div>
      {/if}
    </div>
  </div>
{/if}

{#if app.notice}
  {#key app.notice}
    <div class="notice" class:error={app.notice.kind === "error"} transition:fly={{ y: 16, duration: 200 }}>
      {app.notice.text}
    </div>
  {/key}
{/if}

<Dialogs />

<style>
  .app {
    display: flex;
    flex-direction: column;
    height: 100vh;
  }
  main {
    flex: 1;
    min-height: 0;
  }
  .drop {
    position: fixed;
    inset: 46px 8px 8px;
    z-index: 30;
    display: grid;
    place-items: center;
    border: 2px dashed var(--accent);
    border-radius: var(--radius);
    background: var(--accent-soft);
    backdrop-filter: blur(2px);
    pointer-events: none;
  }
  .drop-card {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    color: var(--accent);
    font-weight: 600;
  }
  .notice {
    position: fixed;
    left: 12px;
    right: 12px;
    bottom: 40px;
    z-index: 60;
    width: fit-content;
    max-width: 520px;
    margin: 0 auto;
    padding: 10px 14px;
    border-radius: 10px;
    background: var(--bg-elevated);
    box-shadow: var(--shadow);
    font-size: 13px;
    word-break: break-word;
  }
  .notice.error {
    border-left: 3px solid var(--red);
  }
</style>
