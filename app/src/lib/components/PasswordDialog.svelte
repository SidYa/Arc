<script lang="ts">
  import { Eye, EyeOff, KeyRound } from "@lucide/svelte";
  import { t } from "../i18n.svelte";
  import Modal from "./Modal.svelte";

  interface Props {
    archive: string;
    wrong: boolean;
    done: (password: string | null) => void;
  }
  let { archive, wrong, done }: Props = $props();

  let password = $state("");
  let reveal = $state(false);

  const submit = () => password && done(password);
</script>

<Modal title={t("password_h")} oncancel={() => done(null)} onconfirm={submit}>
  <div class="head">
    <span class="key"><KeyRound size={20} /></span>
    <p>{t("password_b", archive)}</p>
  </div>
  <div class="pw">
    <input class="input" class:bad={wrong} type={reveal ? "text" : "password"} placeholder={t("password")} bind:value={password} />
    <button class="eye" title={reveal ? t("hide") : t("show")} onclick={() => (reveal = !reveal)}>
      {#if reveal}<EyeOff size={15} />{:else}<Eye size={15} />{/if}
    </button>
  </div>
  {#if wrong}<div class="error">{t("password_wrong")}</div>{/if}
  {#snippet footer()}
    <button class="btn" onclick={() => done(null)}>{t("cancel")}</button>
    <button class="btn primary" disabled={!password} onclick={submit}>{t("ok")}</button>
  {/snippet}
</Modal>

<style>
  .head {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 12px;
  }
  .key {
    display: grid;
    place-items: center;
    flex-shrink: 0;
    width: 40px;
    height: 40px;
    border-radius: 10px;
    background: var(--accent-soft);
    color: var(--accent);
  }
  p {
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
</style>
