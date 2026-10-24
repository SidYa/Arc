<script lang="ts">
  import { Lock } from "@lucide/svelte";
  import { formatSize } from "../format";
  import { t } from "../i18n.svelte";
  import { app, selection, view } from "../store.svelte";

  const archive = $derived(view.archive);
  const selected = $derived.by(() => {
    if (!archive || !selection.size) return null;
    const nodes = (archive.tree.children.get(view.cwd) ?? []).filter((n) => selection.has(n.path));
    return { count: nodes.length, size: nodes.reduce((s, n) => s + n.size, 0) };
  });
  const ratio = $derived(archive && archive.tree.size ? Math.round((archive.listing.size / archive.tree.size) * 100) : null);
</script>

<footer class="status">
  {#if archive}
    <span class="badge">{archive.listing.format.toUpperCase()}</span>
    {#if archive.listing.encrypted}<span class="lock" title={t("encrypted")}><Lock size={12} /></span>{/if}
    <span>{t("files_count", archive.tree.files)}</span>
    <span>{formatSize(archive.tree.size)}</span>
    {#if ratio != null && archive.listing.size}<span title={t("ratio")}>{formatSize(archive.listing.size)} · {ratio}%</span>{/if}
    {#if archive.listing.solid && archive.listing.format !== "tar.gz" && archive.listing.format !== "tar.xz"}<span>{t("solid")}</span>{/if}
    {#if selected}
      <span class="selected">{t("selected", selected.count, formatSize(selected.size))}</span>
    {/if}
  {:else}
    <span>Arc {app.version}</span>
  {/if}
</footer>

<style>
  .status {
    display: flex;
    align-items: center;
    gap: 14px;
    height: 30px;
    padding: 0 14px;
    flex-shrink: 0;
    border-top: 1px solid var(--border);
    font-size: 12px;
    color: var(--text-3);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
    overflow: hidden;
  }
  .badge {
    padding: 1px 6px;
    border-radius: 5px;
    background: var(--accent-soft);
    color: var(--accent);
    font-weight: 700;
    font-size: 11px;
    letter-spacing: 0.3px;
  }
  .lock {
    display: inline-flex;
    margin-left: -8px;
    color: var(--accent);
  }
  .selected {
    margin-left: auto;
    color: var(--text-2);
  }
</style>
