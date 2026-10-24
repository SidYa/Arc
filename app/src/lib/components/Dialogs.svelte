<!-- Shows the first dialog of the queue in store.svelte.ts. -->
<script lang="ts">
  import { t } from "../i18n.svelte";
  import { app, dialogs, type OpenDialog } from "../store.svelte";
  import Modal from "./Modal.svelte";
  import PasswordDialog from "./PasswordDialog.svelte";
  import Settings from "./Settings.svelte";

  const current = $derived(dialogs[0] as OpenDialog | undefined);
  // Narrowing helper: the dialog's kind decides its props and result type
  const as = <K extends OpenDialog["kind"]>(d: OpenDialog, _kind: K) => d as unknown as OpenDialog<K>;
</script>

{#if current}
  {#key current.id}
    {#if current.kind === "confirm"}
      {@const d = as(current, "confirm")}
      <Modal title={d.props.title} oncancel={() => d.resolve(false)} onconfirm={() => d.resolve(true)}>
        <p class="text">{d.props.body}</p>
        {#snippet footer()}
          <button class="btn" onclick={() => d.resolve(false)}>{t("cancel")}</button>
          <button class="btn {d.props.danger ? 'danger' : 'primary'}" onclick={() => d.resolve(true)}>{d.props.action}</button>
        {/snippet}
      </Modal>
    {:else if current.kind === "message"}
      {@const d = as(current, "message")}
      <Modal title={d.props.title} oncancel={() => d.resolve()} onconfirm={() => d.resolve()}>
        <p class="text selectable">{d.props.body}</p>
        {#snippet footer()}
          <button class="btn primary" onclick={() => d.resolve()}>{t("close")}</button>
        {/snippet}
      </Modal>
    {:else if current.kind === "password"}
      {@const d = as(current, "password")}
      <PasswordDialog archive={d.props.archive} wrong={d.props.wrong} done={d.resolve} />
    {:else if current.kind === "settings"}
      {@const d = as(current, "settings")}
      <Settings onclose={() => d.resolve()} />
    {:else if current.kind === "about"}
      {@const d = as(current, "about")}
      <Modal title={t("about")} oncancel={() => d.resolve()} onconfirm={() => d.resolve()}>
        <div class="about">
          <img src="/favicon.png" alt="" width="64" height="64" />
          <div class="about-name">Arc</div>
          <div class="muted">{app.version}</div>
          <div class="muted small">{t("about_desc")}</div>
          <div class="muted small formats">ZIP · 7Z · TAR.GZ · TAR.XZ · RAR</div>
        </div>
        {#snippet footer()}
          <button class="btn primary" onclick={() => d.resolve()}>{t("close")}</button>
        {/snippet}
      </Modal>
    {/if}
  {/key}
{/if}

<style>
  .text {
    margin: 0;
    color: var(--text-2);
    word-break: break-word;
  }
  .selectable {
    user-select: text;
    -webkit-user-select: text;
  }
  .about {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    padding: 8px 0;
    text-align: center;
  }
  .about-name {
    margin-top: 6px;
    font-size: 20px;
    font-weight: 700;
  }
  .formats {
    margin-top: 10px;
    letter-spacing: 0.4px;
  }
</style>
