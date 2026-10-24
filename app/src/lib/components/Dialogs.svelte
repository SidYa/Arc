<!-- Shows the first dialog of the queue in store.svelte.ts. -->
<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { Heart, ScrollText } from "@lucide/svelte";
  import { t } from "../i18n.svelte";
  import { app, dialogs, type OpenDialog } from "../store.svelte";
  import LicensesDialog from "./LicensesDialog.svelte";
  import Modal from "./Modal.svelte";
  import PasswordDialog from "./PasswordDialog.svelte";
  import Settings from "./Settings.svelte";

  const current = $derived(dialogs[0] as OpenDialog | undefined);
  /** About > Licenses, shown on top of About */
  let licenses = $state(false);
  $effect(() => {
    if (current?.kind !== "about") licenses = false;
  });
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
          <div class="muted small">© 2026 Sergii Sidko</div>
        </div>
        {#snippet footer()}
          <!-- PayPal on the left, Licenses and Close on the right, as in Farba -->
          <button class="paypal" onclick={() => void invoke("open_donate")}><Heart size={15} />PayPal</button>
          <button class="btn" onclick={() => (licenses = true)}><ScrollText size={15} />{t("licenses")}</button>
          <button class="btn primary" onclick={() => d.resolve()}>{t("close")}</button>
        {/snippet}
      </Modal>
      {#if licenses}
        <LicensesDialog onclose={() => (licenses = false)} />
      {/if}
    {:else if current.kind === "licenses"}
      {@const d = as(current, "licenses")}
      <LicensesDialog onclose={() => d.resolve()} />
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
  .paypal {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 32px;
    margin-right: auto;
    padding: 0 16px;
    border: none;
    border-radius: 16px;
    background: var(--paypal);
    color: var(--paypal-text);
    font-weight: 600;
    transition: background 0.15s, transform 0.1s;
  }
  .paypal:hover {
    background: var(--paypal-hover);
  }
  .paypal:active {
    transform: scale(0.97);
  }
</style>
