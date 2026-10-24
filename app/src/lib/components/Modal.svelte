<script lang="ts">
  import type { Snippet } from "svelte";
  import { onMount } from "svelte";
  import { fade, scale } from "svelte/transition";
  import { X } from "@lucide/svelte";

  interface Props {
    title: string;
    children: Snippet;
    footer?: Snippet;
    /** Escape, the × button and a click on the backdrop */
    oncancel: () => void;
    /** Enter outside of text areas */
    onconfirm?: () => void;
    wide?: boolean;
  }
  let { title, children, footer, oncancel, onconfirm, wide = false }: Props = $props();

  let card: HTMLDivElement;

  onMount(() => {
    // Focus the first field, or the primary button
    const target =
      card.querySelector<HTMLElement>("input, textarea, select") ?? card.querySelector<HTMLElement>(".btn.primary, .btn.danger");
    target?.focus();
  });

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      oncancel();
    } else if (e.key === "Enter" && onconfirm && !(e.target instanceof HTMLTextAreaElement) && !(e.target instanceof HTMLButtonElement)) {
      e.preventDefault();
      onconfirm();
    }
    e.stopPropagation();
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="backdrop" transition:fade={{ duration: 150 }} onpointerdown={(e) => e.target === e.currentTarget && oncancel()}>
  <div
    class="card"
    class:wide
    role="dialog"
    aria-modal="true"
    aria-label={title}
    tabindex="-1"
    bind:this={card}
    {onkeydown}
    transition:scale={{ start: 0.95, duration: 180 }}
  >
    <div class="head">
      <h2>{title}</h2>
      <button class="close" aria-label="Close" onclick={oncancel}><X size={16} /></button>
    </div>
    <div class="body">
      {@render children()}
    </div>
    {#if footer}
      <div class="foot">
        {@render footer()}
      </div>
    {/if}
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 40;
    /* Flex, not grid: the card's max-height must resolve against the window so .body can scroll */
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 14px;
    background: var(--backdrop);
    backdrop-filter: blur(3px);
  }
  .card {
    display: flex;
    flex-direction: column;
    width: 100%;
    max-width: 420px;
    max-height: 100%;
    border-radius: var(--radius);
    background: var(--bg-elevated);
    box-shadow: var(--shadow);
    outline: none;
  }
  .card.wide {
    max-width: 560px;
  }
  .head {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    padding: 16px 12px 4px 18px;
  }
  h2 {
    flex: 1;
    margin: 0;
    font-size: 16px;
    font-weight: 600;
    line-height: 1.35;
    word-break: break-word;
  }
  .close {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    margin-top: -3px;
    border: none;
    border-radius: 7px;
    background: none;
    color: var(--text-2);
  }
  .close:hover {
    background: var(--surface-hover);
    color: var(--text);
  }
  .body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 8px 18px 16px;
  }
  .foot {
    display: flex;
    justify-content: flex-end;
    flex-wrap: wrap;
    gap: 8px;
    padding: 12px 18px;
    border-top: 1px solid var(--border);
    background: var(--surface);
    border-radius: 0 0 var(--radius) var(--radius);
  }

  /* Compact window: dialogs use the whole window below the title bar */
  @media (max-width: 460px) {
    .backdrop {
      padding: 46px 6px 6px;
    }
    .card,
    .card.wide {
      max-width: none;
    }
    .head {
      padding: 12px 8px 2px 14px;
    }
    h2 {
      font-size: 15px;
    }
    .body {
      padding: 6px 14px 12px;
    }
    .foot {
      padding: 10px 14px;
    }
  }
</style>
