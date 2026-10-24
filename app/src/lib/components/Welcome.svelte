<!-- No archive open: what can be dropped here, and the two main actions. -->
<script lang="ts">
  import { FolderOpen, LoaderCircle, PackagePlus } from "@lucide/svelte";
  import * as actions from "../actions";
  import { baseName } from "../format";
  import { t } from "../i18n.svelte";
  import { app } from "../store.svelte";

  const FORMATS = ["ZIP", "7Z", "TAR.GZ", "TAR.XZ", "RAR"];
</script>

<div class="welcome" data-tauri-drag-region>
  {#if app.loading}
    <div class="loading">
      <LoaderCircle size={28} class="spin" />
      <div>{t("reading", baseName(app.loading))}</div>
    </div>
  {:else}
    <div class="drop">
      <img src="/favicon.png" alt="" width="64" height="64" />
      <div class="headline">{t("welcome_h")}</div>
      <div class="hint">{t("welcome_b")}</div>
      <div class="buttons">
        <button class="btn primary" onclick={actions.pickArchive}><FolderOpen size={16} />{t("open_archive")}</button>
        <button class="btn" onclick={actions.pickFilesToArchive}><PackagePlus size={16} />{t("create_archive")}</button>
      </div>
      <div class="formats">
        {#each FORMATS as f (f)}<span>{f}</span>{/each}
      </div>
    </div>
  {/if}
</div>

<style>
  .welcome {
    display: grid;
    place-items: center;
    height: 100%;
    padding: 0 14px 14px;
  }
  .drop {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 6px;
    width: 100%;
    height: 100%;
    padding: 24px;
    border: 2px dashed var(--border-strong);
    border-radius: var(--radius);
    text-align: center;
  }
  .drop img {
    margin-bottom: 8px;
    filter: drop-shadow(0 6px 16px rgba(0, 0, 0, 0.25));
  }
  .headline {
    font-size: 18px;
    font-weight: 600;
  }
  .hint {
    max-width: 380px;
    color: var(--text-2);
  }
  .buttons {
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: 8px;
    margin-top: 14px;
  }
  .formats {
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: 6px;
    margin-top: 18px;
  }
  .formats span {
    padding: 2px 8px;
    border-radius: 6px;
    background: var(--surface-active);
    color: var(--text-3);
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.4px;
  }
  .loading {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    color: var(--text-2);
  }
  .loading :global(.spin) {
    color: var(--accent);
    animation: spin 0.9s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
