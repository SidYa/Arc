<!-- New archive inside the task window: name and folder, format, compression, optional password. -->
<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { Eye, EyeOff, File, Folder, FolderOpen, ShieldCheck, TriangleAlert } from "@lucide/svelte";
  import { onMount, untrack } from "svelte";
  import { api, supportsPassword, WRITABLE, type CreateOptions, type Format } from "../api";
  import { baseName, dirName, extensionOf } from "../format";
  import { t } from "../i18n.svelte";
  import { app } from "../store.svelte";
  import Select from "./Select.svelte";

  interface Props {
    sources: string[];
    password: boolean;
    done: (result: { dest: string; options: CreateOptions } | null) => void;
  }
  let { sources, password: withPassword, done }: Props = $props();

  // Password protection needs a format that has it
  const startFormat = (): Format => {
    const f = app.config?.default_format ?? "zip";
    return withPassword && !supportsPassword(f) ? "7z" : f;
  };
  let format = $state<Format>(startFormat());
  let level = $state(app.config?.default_level ?? 5);
  let folder = $state("");
  let name = $state("");
  let protect = $state(untrack(() => withPassword));
  let password = $state("");
  let confirm = $state("");
  let reveal = $state(false);
  let encryptNames = $state(true);
  let infos = $state<{ name: string; is_dir: boolean }[]>([]);
  let exists = $state(false);
  let nameInput = $state<HTMLInputElement>();
  let passwordInput = $state<HTMLInputElement>();

  onMount(async () => {
    const suggested = await api.suggestArchive(sources, format);
    folder = dirName(suggested);
    const file = baseName(suggested);
    name = file.slice(0, file.length - extensionOf(suggested).length - 1);
    // "Add with password": the password is what the user came for
    setTimeout(() => (protect ? passwordInput : nameInput)?.focus(), 0);
    infos = await api.pathInfo(sources);
  });

  const canProtect = $derived(supportsPassword(format));
  const mismatch = $derived(protect && confirm.length > 0 && password !== confirm);
  const valid = $derived(name.trim().length > 0 && folder.length > 0 && (!protect || !canProtect || (password.length > 0 && password === confirm)));
  const dest = $derived(`${folder}${folder.endsWith("\\") || folder.endsWith("/") ? "" : "\\"}${name.trim()}.${format}`);

  // An existing archive gets replaced: say so before it happens
  $effect(() => {
    const path = dest;
    if (!folder || !name.trim()) return;
    api.exists(path).then((e) => path === dest && (exists = e));
  });

  const LEVELS = $derived([
    { value: 0, label: t("level_store") },
    { value: 1, label: t("level_fast") },
    { value: 5, label: t("level_normal") },
    { value: 7, label: t("level_max") },
    { value: 9, label: t("level_ultra") },
  ]);

  async function chooseFolder() {
    const dir = await open({ directory: true, defaultPath: folder });
    if (typeof dir === "string") folder = dir;
  }

  function submit() {
    if (!valid) return;
    done({
      dest,
      options: {
        format,
        level,
        password: protect && canProtect ? password : null,
        encrypt_names: protect && format === "7z" && encryptNames,
      },
    });
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && !(e.target instanceof HTMLButtonElement)) {
      e.preventDefault();
      submit();
    } else if (e.key === "Escape") {
      e.preventDefault();
      done(null);
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="form" {onkeydown}>
  <div class="sources">
    {#each infos.slice(0, 3) as info (info.name)}
      <span class="chip">
        {#if info.is_dir}<Folder size={13} />{:else}<File size={13} />{/if}
        <span class="ellipsis">{info.name}</span>
      </span>
    {/each}
    {#if sources.length > 3}<span class="chip">+{sources.length - 3}</span>{/if}
  </div>

  <div class="field">
    <span class="label">{t("archive_name")}</span>
    <div class="name">
      <input class="input" bind:this={nameInput} bind:value={name} spellcheck="false" />
      <span class="ext">.{format}</span>
    </div>
    {#if exists}<div class="warn"><TriangleAlert size={13} />{t("exists_replace")}</div>{/if}
  </div>

  <div class="field">
    <span class="label">{t("folder")}</span>
    <div class="folder">
      <span class="path ellipsis" title={folder}>{folder}</span>
      <button class="btn small-btn" onclick={chooseFolder}><FolderOpen size={14} />{t("choose")}</button>
    </div>
  </div>

  <div class="row2">
    <div class="field">
      <span class="label">{t("format")}</span>
      <div class="segmented">
        {#each WRITABLE as f (f)}
          <button class:active={format === f} onclick={() => (format = f)}>{f.toUpperCase()}</button>
        {/each}
      </div>
    </div>
    <div class="field grow">
      <span class="label">{t("level")}</span>
      <Select value={level} options={LEVELS} onchange={(v) => (level = v)} />
    </div>
  </div>

  <div class="protect" class:on={protect && canProtect}>
    <label class="toggle-row">
      <ShieldCheck size={17} />
      <div class="toggle-label">
        <div>{t("protect")}</div>
        <div class="desc">{canProtect ? (format === "zip" ? t("protect_zip_desc") : t("protect_7z_desc")) : t("protect_unsupported")}</div>
      </div>
      <input type="checkbox" class="switch" bind:checked={protect} disabled={!canProtect} />
    </label>
    {#if protect && canProtect}
      <div class="passwords">
        <div class="pw">
          <input
            class="input"
            bind:this={passwordInput}
            type={reveal ? "text" : "password"}
            placeholder={t("password")}
            bind:value={password}
            autocomplete="new-password"
          />
          <button class="eye" title={reveal ? t("hide") : t("show")} onclick={() => (reveal = !reveal)}>
            {#if reveal}<EyeOff size={15} />{:else}<Eye size={15} />{/if}
          </button>
        </div>
        <input
          class="input"
          class:bad={mismatch}
          type={reveal ? "text" : "password"}
          placeholder={t("password_repeat")}
          bind:value={confirm}
          autocomplete="new-password"
        />
        {#if mismatch}<div class="error">{t("password_mismatch")}</div>{/if}
        {#if format === "7z"}
          <label class="check">
            <input type="checkbox" bind:checked={encryptNames} />
            {t("encrypt_names")}
          </label>
        {/if}
      </div>
    {/if}
  </div>

  <div class="buttons">
    <button class="btn" onclick={() => done(null)}>{t("cancel")}</button>
    <button class="btn primary" disabled={!valid} onclick={submit}>{exists ? t("overwrite") : t("create")}</button>
  </div>
</div>

<style>
  .form {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .sources {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    max-width: 190px;
    padding: 3px 9px;
    border-radius: 999px;
    background: var(--surface-active);
    color: var(--text-2);
    font-size: 12px;
  }
  .chip :global(svg) {
    flex-shrink: 0;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 5px;
    min-width: 0;
  }
  .label {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-2);
  }
  .name,
  .folder {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .ext {
    flex-shrink: 0;
    color: var(--text-3);
    font-weight: 600;
  }
  .warn {
    display: flex;
    align-items: center;
    gap: 5px;
    font-size: 12px;
    color: var(--accent-orange);
  }
  .path {
    flex: 1;
    min-width: 0;
    font-size: 13px;
    color: var(--text-2);
  }
  .small-btn {
    height: 30px;
    padding: 0 10px;
    font-size: 13px;
  }
  .row2 {
    display: flex;
    gap: 14px;
  }
  .grow {
    flex: 1;
  }
  .row2 :global(.select) {
    width: 100%;
    min-width: 0;
  }
  .segmented {
    display: flex;
    align-self: flex-start;
    padding: 2px;
    border-radius: 8px;
    background: var(--surface-active);
  }
  .segmented button {
    height: 28px;
    padding: 0 9px;
    border: none;
    border-radius: 6px;
    background: none;
    color: var(--text-2);
    font-size: 12px;
    font-weight: 600;
    transition: background 0.15s, color 0.15s;
  }
  .segmented button.active {
    background: var(--bg-elevated);
    color: var(--text);
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.2);
  }
  .protect {
    border: 1px solid var(--border);
    border-radius: 10px;
    background: var(--surface);
    transition: border-color 0.15s;
  }
  .protect.on {
    border-color: var(--accent-soft-strong);
  }
  .toggle-row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 9px 12px;
  }
  .toggle-row > :global(svg) {
    flex-shrink: 0;
    color: var(--accent);
  }
  .toggle-label {
    flex: 1;
    min-width: 0;
  }
  .desc {
    font-size: 12px;
    color: var(--text-2);
  }
  .passwords {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 0 12px 12px;
  }
  .pw {
    position: relative;
  }
  .pw .input {
    padding-right: 36px;
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
  .input.bad {
    border-color: var(--red);
  }
  .error {
    font-size: 12px;
    color: var(--red);
  }
  .check {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
    color: var(--text-2);
  }
  .check input {
    accent-color: var(--accent);
  }
  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 4px;
  }
</style>
