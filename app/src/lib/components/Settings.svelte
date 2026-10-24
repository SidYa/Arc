<!-- Preferences: every change is applied and saved immediately, as in Tormin. -->
<script lang="ts">
  import { Check, Link, Unlink } from "@lucide/svelte";
  import { onMount } from "svelte";
  import { api, WRITABLE, type Association, type Config } from "../api";
  import { languages, setLanguage, t } from "../i18n.svelte";
  import { app, notify } from "../store.svelte";
  import Modal from "./Modal.svelte";
  import Select from "./Select.svelte";

  // File associations: what Windows opens each archive type with right now
  let associations = $state<Association[]>([]);
  let associating = $state(false);
  const allArc = $derived(associations.length > 0 && associations.every((a) => a.is_arc));
  const anyArc = $derived(associations.some((a) => a.is_arc));

  onMount(async () => {
    if (app.platform === "windows") associations = await api.associations();
  });

  async function associate() {
    associating = true;
    try {
      associations = await api.associate();
      // Windows keeps some choices for itself: Arc's page in Settings is open for those
      notify(associations.every((a) => a.is_arc) ? t("assoc_done") : t("assoc_settings"));
    } catch (e) {
      notify(String(e), "error");
    } finally {
      associating = false;
    }
  }

  async function unassociate() {
    try {
      associations = await api.unassociate();
    } catch (e) {
      notify(String(e), "error");
    }
  }

  // Back from Settings: the choices made there show up
  function onfocus() {
    if (app.platform === "windows" && associations.length) api.associations().then((a) => (associations = a));
  }

  interface Props {
    onclose: () => void;
  }
  let { onclose }: Props = $props();

  const config = $derived(app.config!);
  const LANGUAGE_NAMES: Record<string, string> = { en: "English", uk: "Українська" };
  const ACCENTS = ["purple", "green", "orange", "blue"];
  const custom = $derived(config.accent.startsWith("#"));

  async function update<K extends keyof Config>(key: K, value: Config[K]) {
    const next = { ...config, [key]: value };
    app.config = next;
    if (key === "language") setLanguage(value as string);
    try {
      await api.setConfig(next);
    } catch (e) {
      notify(String(e), "error");
    }
  }
</script>

{#snippet toggle(key: keyof Config, title: string, description?: string)}
  <label class="row">
    <div class="label">
      <div>{title}</div>
      {#if description}<div class="desc">{description}</div>{/if}
    </div>
    <input
      type="checkbox"
      class="switch"
      checked={config[key] as boolean}
      onchange={(e) => update(key, e.currentTarget.checked as never)}
    />
  </label>
{/snippet}

<svelte:window {onfocus} />

<Modal title={t("preferences")} wide oncancel={onclose}>
  <section>
    <h3>{t("appearance")}</h3>
    <div class="card">
      <div class="row">
        <div class="label">
          <div>{t("theme")}</div>
          <div class="desc">{t("theme_desc")}</div>
        </div>
        <div class="segmented">
          {#each [["system", t("theme_system")], ["light", t("theme_light")], ["dark", t("theme_dark")]] as [value, label] (value)}
            <button class:active={config.theme === value} onclick={() => update("theme", value as Config["theme"])}>{label}</button>
          {/each}
        </div>
      </div>
      <div class="row">
        <div class="label">
          <div>{t("accent_color")}</div>
          <div class="desc">{t("accent_desc")}</div>
        </div>
        <div class="swatches" class:off={config.monochrome} title={config.monochrome ? t("accent_mono") : undefined}>
          {#if app.systemAccent}
            <button
              class="swatch system"
              class:active={config.accent === "system"}
              style:--swatch={app.systemAccent}
              title={t("accent_system")}
              aria-label={t("accent_system")}
              onclick={() => update("accent", "system")}
            ></button>
            <span class="divider"></span>
          {/if}
          {#each ACCENTS as value (value)}
            <button
              class="swatch"
              class:active={config.accent === value}
              style:--swatch="var(--accent-{value})"
              title={t(`accent_${value}`)}
              aria-label={t(`accent_${value}`)}
              onclick={() => update("accent", value)}
            ></button>
          {/each}
          <label class="swatch custom" class:active={custom} style:--swatch={custom ? config.accent : undefined} title={t("accent_custom")}>
            <input type="color" value={custom ? config.accent : "#8f7dff"} onchange={(e) => update("accent", e.currentTarget.value)} />
          </label>
        </div>
      </div>
      {@render toggle("monochrome", t("monochrome"), t("monochrome_desc"))}
      <div class="row">
        <div class="label"><div>{t("language")}</div></div>
        <Select
          value={config.language}
          options={languages.map((code) => ({ value: code, label: LANGUAGE_NAMES[code] ?? code.toUpperCase() }))}
          onchange={(v) => update("language", v)}
        />
      </div>
    </div>
  </section>

  <section>
    <h3>{t("archiving")}</h3>
    <div class="card">
      <div class="row">
        <div class="label">
          <div>{t("default_format")}</div>
          <div class="desc">{t("default_format_desc")}</div>
        </div>
        <Select
          value={config.default_format}
          options={WRITABLE.map((f) => ({ value: f, label: f.toUpperCase() }))}
          onchange={(v) => update("default_format", v)}
        />
      </div>
      <div class="row">
        <div class="label"><div>{t("level")}</div></div>
        <Select
          value={config.default_level}
          options={[
            { value: 0, label: t("level_store") },
            { value: 1, label: t("level_fast") },
            { value: 5, label: t("level_normal") },
            { value: 7, label: t("level_max") },
            { value: 9, label: t("level_ultra") },
          ]}
          onchange={(v) => update("default_level", v)}
        />
      </div>
      <div class="row">
        <div class="label">
          <div>{t("conflict")}</div>
          <div class="desc">{t("conflict_desc")}</div>
        </div>
        <Select
          value={config.conflict}
          options={[
            { value: "rename", label: t("conflict_rename") },
            { value: "overwrite", label: t("conflict_overwrite") },
            { value: "skip", label: t("conflict_skip") },
          ]}
          onchange={(v) => update("conflict", v as Config["conflict"])}
        />
      </div>
      {@render toggle("open_folder_after", t("open_folder_after"), t("open_folder_after_desc"))}
    </div>
  </section>

  {#if app.platform === "windows"}
    <section>
      <h3>Windows</h3>
      <div class="card">
        <div class="row">
          <div class="label">
            <div>{t("assoc")}</div>
            <div class="desc">{t("assoc_desc")}</div>
          </div>
          <div class="assoc-buttons">
            {#if anyArc}
              <button class="btn" title={t("assoc_remove")} aria-label={t("assoc_remove")} onclick={unassociate}><Unlink size={15} /></button>
            {/if}
            <button class="btn" class:primary={!allArc} disabled={associating} onclick={associate}>
              {#if allArc}<Check size={15} />{t("assoc_all_done")}{:else}<Link size={15} />{t("assoc_make_default")}{/if}
            </button>
          </div>
          {#if associations.length}
            <div class="exts">
              {#each associations as a (a.ext)}
                <span class="ext" class:on={a.is_arc} title={a.is_arc ? "Arc" : (a.app ?? t("assoc_none"))}>.{a.ext}</span>
              {/each}
            </div>
          {/if}
        </div>
        {@render toggle("context_menu", t("context_menu"), t("context_menu_desc"))}
      </div>
    </section>
  {/if}

  {#snippet footer()}
    <button class="btn primary" onclick={onclose}>{t("close")}</button>
  {/snippet}
</Modal>

<style>
  section + section {
    margin-top: 18px;
  }
  h3 {
    margin: 0 0 8px 2px;
    font-size: 13px;
    font-weight: 600;
    color: var(--text-2);
  }
  .card {
    border: 1px solid var(--border);
    border-radius: 10px;
    background: var(--surface);
    overflow: hidden;
  }
  /* Controls move below their label when the window is too narrow for both */
  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px 12px;
    min-height: 52px;
    padding: 9px 12px;
  }
  .row + .row {
    border-top: 1px solid var(--border);
  }
  .label {
    flex: 1 1 170px;
    min-width: 0;
  }
  .desc {
    margin-top: 1px;
    font-size: 12px;
    color: var(--text-2);
  }
  .segmented {
    display: flex;
    flex-shrink: 0;
    padding: 2px;
    border-radius: 8px;
    background: var(--surface-active);
  }
  .segmented button {
    height: 28px;
    padding: 0 10px;
    border: none;
    border-radius: 6px;
    background: none;
    color: var(--text-2);
    font-size: 12.5px;
    transition: background 0.15s, color 0.15s;
  }
  .segmented button.active {
    background: var(--bg-elevated);
    color: var(--text);
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.2);
  }

  .swatches {
    display: flex;
    align-items: center;
    flex-shrink: 0;
    gap: 8px;
    padding: 3px;
  }
  /* Monochrome overrides the accent: the colors stay selectable for later */
  .swatches.off {
    opacity: 0.4;
  }
  .divider {
    width: 1px;
    height: 18px;
    background: var(--border-strong);
  }
  /* Filled circle with a ring; the selected one gets a second ring in its own color */
  .swatch {
    position: relative;
    width: 22px;
    height: 22px;
    padding: 0;
    border: 2px solid var(--border-strong);
    border-radius: 50%;
    background: var(--swatch);
    background-clip: padding-box;
    transition: transform 0.12s, box-shadow 0.15s;
  }
  .swatch:hover {
    transform: scale(1.12);
  }
  .swatch.active {
    border-color: var(--bg-elevated);
    box-shadow: 0 0 0 2px var(--swatch);
  }
  .assoc-buttons {
    display: flex;
    gap: 6px;
    flex-shrink: 0;
  }
  .exts {
    display: flex;
    flex-wrap: wrap;
    gap: 5px;
    width: 100%;
  }
  .ext {
    padding: 1px 7px;
    border-radius: 6px;
    background: var(--surface-active);
    color: var(--text-3);
    font-size: 11.5px;
    font-weight: 600;
  }
  .ext.on {
    background: var(--accent-soft);
    color: var(--accent);
  }
  /* The Windows accent: an inner ring tells it from the presets */
  .swatch.system::after {
    content: "";
    position: absolute;
    inset: 4px;
    border-radius: 50%;
    border: 2px solid rgba(255, 255, 255, 0.85);
  }
  .swatch.custom:not(.active) {
    background: conic-gradient(#ff5c5c, #f0c419, #22b573, #2fa8e8, #8f7dff, #ff5c9d, #ff5c5c);
  }
  .swatch.custom input {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    opacity: 0;
    cursor: pointer;
  }
</style>
