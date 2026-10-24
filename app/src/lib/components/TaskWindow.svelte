<!-- A small window for one task: archive (with its options form) or extract one or more archives.
     Shows the progress with Pause and Cancel, asks for passwords, closes itself when done. -->
<script lang="ts">
  import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
  import { listen } from "@tauri-apps/api/event";
  import { CircleAlert, Eye, EyeOff, KeyRound, Minus, PackageOpen, PackagePlus, Pause, Play, X } from "@lucide/svelte";
  import { onMount, tick } from "svelte";
  import { api, type CreateOptions, type JobFinished, type JobProgress, type JobSpec, type Task } from "../api";
  import { baseName, formatEta, formatSize, formatSpeed } from "../format";
  import { t } from "../i18n.svelte";
  import { errorText, isPasswordError, onJobFinished, onJobProgress, startJob, type Job } from "../jobs";
  import { app } from "../store.svelte";
  import CreateForm from "./CreateForm.svelte";

  const win = getCurrentWindow();
  const taskId = Number(win.label.slice("task-".length));

  type Stage = "loading" | "form" | "password" | "progress" | "result";
  let stage = $state<Stage>("loading");
  let task = $state<Task | null>(null);
  let queue = $state<JobSpec[]>([]);
  let index = $state(0);
  let job = $state<Job | null>(null);
  let paused = $state(false);
  let cancelled = false;
  let errors = $state<{ name: string; text: string }[]>([]);

  // Password prompt
  let ask = $state<{ archive: string; wrong: boolean; resolve: (p: string | null) => void } | null>(null);
  let password = $state("");
  let reveal = $state(false);
  let passwordInput = $state<HTMLInputElement>();

  const creating = $derived(task?.action === "add" || task?.action === "add_password" || queue[index]?.kind === "create");
  const spec = $derived(queue[index]);
  const name = $derived(spec ? baseName(spec.kind === "create" ? spec.dest : spec.archive) : "");
  const percent = $derived(job && job.total > 0 ? Math.min(100, (job.done / job.total) * 100) : 0);

  // ---------------------------------------------------------------- window size follows the content

  let box: HTMLDivElement;
  let shown = false;
  const WIDTH = 440;
  const FORM_WIDTH = 480;

  async function fit() {
    if (!box) return;
    const width = stage === "form" ? FORM_WIDTH : WIDTH;
    await win.setSize(new LogicalSize(width, Math.ceil(box.getBoundingClientRect().height)));
    if (!shown && stage !== "loading") {
      shown = true;
      await win.center();
      await win.show();
      await win.setFocus();
    }
  }

  onMount(() => {
    const observer = new ResizeObserver(() => fit());
    observer.observe(box);
    const unlisten = [
      listen<JobProgress[]>("job-progress", (e) => onJobProgress(e.payload)),
      listen<JobFinished>("job-finished", (e) => onJobFinished(e.payload)),
      listen("close-requested", () => cancel()),
    ];
    start();
    return () => {
      observer.disconnect();
      unlisten.forEach((p) => p.then((f) => f()));
    };
  });

  // ---------------------------------------------------------------- the work

  async function start() {
    task = await api.takeTask(taskId);
    if (!task) return close();
    const paths = task.paths ?? [];
    if (task.action === "add" || task.action === "add_password") {
      stage = "form";
      return;
    }
    if (task.action === "jobs") queue = task.specs ?? [];
    else {
      const mode = task.action === "extract_here" ? "here" : "folder";
      queue = paths.map((archive) => ({ kind: "extract", archive, mode, password: null }));
    }
    await run();
  }

  async function onForm(result: { dest: string; options: CreateOptions } | null) {
    if (!result) return close();
    queue = [{ kind: "create", sources: task?.paths ?? [], dest: result.dest, options: result.options }];
    await run();
  }

  async function run() {
    stage = "progress";
    for (index = 0; index < queue.length; index++) {
      let current = queue[index];
      for (;;) {
        paused = false;
        // The job is tracked through the $state proxy, so progress updates reach the screen
        const started = startJob(current, (j) => {
          job = j;
          return job!;
        });
        const finished = await started.finished;
        if (cancelled || finished.state === "cancelled") return close();
        if (finished.state === "error" && isPasswordError(finished.error) && current.kind !== "create") {
          const pw = await askPassword(baseName(current.archive), finished.error === "wrong_password");
          if (pw == null) break; // this archive is skipped
          current = { ...current, password: pw };
          queue[index] = current;
          stage = "progress";
          continue;
        }
        if (finished.state === "error") errors.push({ name: baseName(current.kind === "create" ? current.dest : current.archive), text: errorText(finished.error) });
        else if (current.kind === "extract" && app.config?.open_folder_after && finished.output) api.reveal(finished.output);
        break;
      }
    }
    if (errors.length) stage = "result";
    else close();
  }

  function askPassword(archive: string, wrong: boolean): Promise<string | null> {
    password = "";
    stage = "password";
    return new Promise((resolve) => {
      ask = { archive, wrong, resolve };
      tick().then(() => passwordInput?.focus());
    });
  }

  function answer(value: string | null) {
    const pending = ask;
    ask = null;
    pending?.resolve(value);
  }

  function togglePause() {
    if (!job || job.state !== "running") return;
    paused = !paused;
    api.pauseJob(job.id, paused);
    // The speed starts over after a pause
    job.speed = 0;
    job.sample = null;
  }

  /** Cancel button, Escape, Alt+F4: the job removes what it started writing */
  function cancel() {
    cancelled = true;
    if (stage === "password") answer(null);
    if (job && job.state === "running") api.cancelJob(job.id);
    else close();
  }

  function close() {
    win.destroy();
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && stage === "progress") {
      e.preventDefault();
      cancel();
    }
  }

  function stats(j: Job): string {
    if (!j.total) return t("preparing");
    const parts = [`${formatSize(j.done)} ${t("of")} ${formatSize(j.total)}`];
    if (paused) parts.push(t("paused"));
    else if (j.speed > 0) {
      parts.push(formatSpeed(j.speed));
      const eta = formatEta((j.total - j.done) / j.speed);
      if (eta) parts.push(t("eta", eta));
    }
    return parts.join(" · ");
  }
</script>

<svelte:window {onkeydown} />

<div class="task" bind:this={box}>
  <header class="bar" data-tauri-drag-region>
    <img src="/favicon.png" alt="" width="16" height="16" data-tauri-drag-region />
    <span class="title ellipsis" data-tauri-drag-region>
      {#if stage === "form"}{task?.action === "add_password" ? t("create_protected_h") : t("create_h")}
      {:else if stage === "password"}{t("password_h")}
      {:else if creating}{t("task_create")}
      {:else}{t("task_extract")}{/if}
    </span>
    <button class="win" title={t("minimize")} aria-label={t("minimize")} onclick={() => win.minimize()}><Minus size={14} /></button>
    <button class="win close" title={t("cancel")} aria-label={t("cancel")} onclick={cancel}><X size={14} /></button>
  </header>

  <div class="content">
    {#if stage === "form" && task}
      <CreateForm sources={task.paths ?? []} password={task.action === "add_password"} done={onForm} />
    {:else if stage === "password" && ask}
      <div class="ask">
        <span class="key"><KeyRound size={18} /></span>
        <p>{t("password_b", ask.archive)}</p>
      </div>
      <form
        onsubmit={(e) => {
          e.preventDefault();
          if (password) answer(password);
        }}
      >
        <div class="pw">
          <input class="input" class:bad={ask.wrong} bind:this={passwordInput} type={reveal ? "text" : "password"} placeholder={t("password")} bind:value={password} />
          <button type="button" class="eye" title={reveal ? t("hide") : t("show")} onclick={() => (reveal = !reveal)}>
            {#if reveal}<EyeOff size={15} />{:else}<Eye size={15} />{/if}
          </button>
        </div>
        {#if ask.wrong}<div class="error">{t("password_wrong")}</div>{/if}
        <div class="buttons">
          <button type="button" class="btn" onclick={() => answer(null)}>{queue.length > 1 ? t("skip") : t("cancel")}</button>
          <button type="submit" class="btn primary" disabled={!password}>{t("ok")}</button>
        </div>
      </form>
    {:else if stage === "progress" && job}
      <div class="head">
        <span class="kind">
          {#if creating}<PackagePlus size={20} />{:else}<PackageOpen size={20} />{/if}
        </span>
        <div class="names">
          <div class="name ellipsis" title={name}>{name}</div>
          <div class="file ellipsis" title={job.current}>{job.current || " "}</div>
        </div>
        {#if queue.length > 1}<span class="count">{t("task_of", index + 1, queue.length)}</span>{/if}
      </div>
      <div class="track" class:paused class:indeterminate={!job.total}>
        <div class="fill" style:width="{percent}%"></div>
      </div>
      <div class="line">
        <span class="stats ellipsis">{stats(job)}</span>
        <span class="pct">{Math.floor(percent)}%</span>
      </div>
      <div class="buttons">
        <button class="btn" onclick={togglePause}>
          {#if paused}<Play size={14} />{t("resume")}{:else}<Pause size={14} />{t("pause")}{/if}
        </button>
        <button class="btn" onclick={cancel}>{t("cancel")}</button>
      </div>
    {:else if stage === "result"}
      <div class="result">
        <span class="bad-icon"><CircleAlert size={20} /></span>
        <div class="errors">
          {#each errors as err (err.name + err.text)}
            <div class="err">
              <div class="name ellipsis">{err.name}</div>
              <div class="text">{err.text}</div>
            </div>
          {/each}
        </div>
      </div>
      <div class="buttons">
        <button class="btn primary" onclick={close}>{t("close")}</button>
      </div>
    {/if}
  </div>
</div>

<style>
  .task {
    display: flex;
    flex-direction: column;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 36px;
    padding-left: 12px;
  }
  .title {
    flex: 1;
    min-width: 0;
    font-size: 12.5px;
    font-weight: 600;
    color: var(--text-2);
  }
  .win {
    display: grid;
    place-items: center;
    width: 40px;
    height: 36px;
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
  .content {
    padding: 6px 18px 16px;
  }

  /* progress */
  .head {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .kind {
    display: grid;
    place-items: center;
    flex-shrink: 0;
    width: 38px;
    height: 38px;
    border-radius: 10px;
    background: var(--accent-soft);
    color: var(--accent);
  }
  .names {
    flex: 1;
    min-width: 0;
  }
  .name {
    font-weight: 600;
  }
  .file {
    font-size: 12px;
    color: var(--text-3);
  }
  .count {
    flex-shrink: 0;
    padding: 2px 8px;
    border-radius: 6px;
    background: var(--surface-active);
    color: var(--text-2);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }
  .track {
    position: relative;
    height: 6px;
    margin: 14px 0 6px;
    border-radius: 3px;
    background: var(--surface-active);
    overflow: hidden;
  }
  .fill {
    height: 100%;
    border-radius: 3px;
    background: var(--accent);
    transition: width 0.2s linear, background 0.2s;
  }
  .track.paused .fill {
    background: var(--text-3);
  }
  .indeterminate .fill {
    width: 30% !important;
    animation: slide 1.1s var(--ease) infinite;
  }
  @keyframes slide {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(340%);
    }
  }
  .line {
    display: flex;
    gap: 10px;
    font-size: 12px;
    color: var(--text-2);
    font-variant-numeric: tabular-nums;
  }
  .stats {
    flex: 1;
    min-width: 0;
  }
  .pct {
    font-weight: 600;
    color: var(--text);
  }
  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 14px;
  }
  .buttons .btn {
    min-width: 96px;
  }

  /* password */
  .ask {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 12px;
  }
  .key {
    display: grid;
    place-items: center;
    flex-shrink: 0;
    width: 38px;
    height: 38px;
    border-radius: 10px;
    background: var(--accent-soft);
    color: var(--accent);
  }
  .ask p {
    margin: 0;
    color: var(--text-2);
    word-break: break-word;
  }
  .pw {
    position: relative;
  }
  .pw .input {
    padding-right: 36px;
  }
  .input.bad {
    border-color: var(--red);
  }
  .eye {
    position: absolute;
    top: 3px;
    right: 3px;
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    border: none;
    border-radius: 6px;
    background: none;
    color: var(--text-2);
  }
  .eye:hover {
    background: var(--surface-hover);
  }
  .error {
    margin-top: 6px;
    font-size: 12px;
    color: var(--red);
  }

  /* result */
  .result {
    display: flex;
    gap: 12px;
  }
  .bad-icon {
    display: grid;
    place-items: center;
    flex-shrink: 0;
    width: 38px;
    height: 38px;
    border-radius: 10px;
    background: var(--red-soft);
    color: var(--red);
  }
  .errors {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-height: 220px;
    overflow-y: auto;
  }
  .err .text {
    font-size: 12px;
    color: var(--red);
    word-break: break-word;
  }
</style>
