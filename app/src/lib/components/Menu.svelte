<!-- Popup menu: title bar buttons, the row context menu, dropdowns. -->
<script lang="ts" module>
  import type { Component } from "svelte";

  export interface MenuItem {
    label?: string;
    icon?: Component<{ size?: number }>;
    shortcut?: string;
    action?: () => void;
    disabled?: boolean;
    danger?: boolean;
    separator?: boolean;
  }
</script>

<script lang="ts">
  import { onMount, tick } from "svelte";

  interface Props {
    items: MenuItem[];
    x: number;
    y: number;
    /** "right" aligns the menu's right edge with x */
    align?: "left" | "right";
    /** Narrower than the default 200px, e.g. to match a dropdown button */
    minWidth?: number;
    onclose: () => void;
  }
  let { items, x, y, align = "left", minWidth, onclose }: Props = $props();

  let menu: HTMLDivElement;
  let left = $state(0);
  let top = $state(0);

  onMount(async () => {
    await tick();
    const rect = menu.getBoundingClientRect();
    const margin = 6;
    left = align === "right" ? x - rect.width : x;
    left = Math.max(margin, Math.min(left, window.innerWidth - rect.width - margin));
    top = y + rect.height > window.innerHeight - margin ? Math.max(margin, y - rect.height) : y;
    menu.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus();
  });

  function run(item: MenuItem) {
    onclose();
    item.action?.();
  }

  function onkeydown(e: KeyboardEvent) {
    const buttons = [...menu.querySelectorAll<HTMLButtonElement>("button:not(:disabled)")];
    const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
    if (e.key === "Escape") onclose();
    else if (e.key === "ArrowDown") buttons[(index + 1) % buttons.length]?.focus();
    else if (e.key === "ArrowUp") buttons[(index - 1 + buttons.length) % buttons.length]?.focus();
    else return;
    e.preventDefault();
    e.stopPropagation();
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="layer"
  onpointerdown={(e) => e.target === e.currentTarget && onclose()}
  oncontextmenu={(e) => {
    e.preventDefault();
    onclose();
  }}
>
  <div
    class="menu"
    role="menu"
    tabindex="-1"
    bind:this={menu}
    style:left="{left}px"
    style:top="{top}px"
    style:min-width={minWidth ? `${minWidth}px` : undefined}
    {onkeydown}
  >
    {#each items as item, i (i)}
      {#if item.separator}
        <div class="sep"></div>
      {:else}
        <button role="menuitem" class:danger={item.danger} disabled={item.disabled} onclick={() => run(item)}>
          <span class="icon">
            {#if item.icon}<item.icon size={16} />{/if}
          </span>
          <span class="label">{item.label}</span>
          {#if item.shortcut}<span class="shortcut">{item.shortcut}</span>{/if}
        </button>
      {/if}
    {/each}
  </div>
</div>

<style>
  .layer {
    position: fixed;
    inset: 0;
    z-index: 50;
  }
  .menu {
    position: fixed;
    min-width: 200px;
    padding: 5px;
    border-radius: 10px;
    background: var(--bg-elevated);
    box-shadow: var(--shadow);
    animation: pop 0.14s var(--ease);
    transform-origin: top;
  }
  button {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    height: 32px;
    padding: 0 10px;
    border: none;
    border-radius: 6px;
    background: none;
    text-align: left;
  }
  button:hover,
  button:focus-visible {
    background: var(--surface-hover);
    outline: none;
  }
  button:disabled {
    opacity: 0.4;
  }
  button.danger {
    color: var(--red);
  }
  .icon {
    display: inline-flex;
    width: 16px;
    color: var(--text-2);
  }
  .danger .icon {
    color: var(--red);
  }
  .label {
    flex: 1;
  }
  .shortcut {
    font-size: 12px;
    color: var(--text-3);
  }
  .sep {
    height: 1px;
    margin: 5px 8px;
    background: var(--border);
  }
  @keyframes pop {
    from {
      opacity: 0;
      transform: scale(0.96) translateY(-4px);
    }
  }
</style>
