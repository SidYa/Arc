<!-- Dropdown drawn by the app itself: the native <select> popup ignores the dark theme on Windows. -->
<script lang="ts" generics="T extends string | number">
  import { Check, ChevronDown } from "@lucide/svelte";
  import Menu, { type MenuItem } from "./Menu.svelte";

  interface Props {
    value: T;
    options: { value: T; label: string }[];
    onchange: (value: T) => void;
  }
  let { value, options, onchange }: Props = $props();

  let menu = $state<{ x: number; y: number; width: number } | null>(null);
  const current = $derived(options.find((o) => o.value === value)?.label ?? "");

  // The list opens right under the button: same left edge, same width
  function toggle(e: MouseEvent) {
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    menu = menu ? null : { x: rect.left, y: rect.bottom + 4, width: rect.width };
  }

  const items = (): MenuItem[] =>
    options.map((o) => ({
      label: o.label,
      // Empty icon slot keeps the labels aligned with the checked one
      icon: o.value === value ? Check : undefined,
      action: () => o.value !== value && onchange(o.value),
    }));
</script>

<button class="select" class:open={menu} onclick={toggle} aria-haspopup="listbox">
  <span class="ellipsis">{current}</span>
  <ChevronDown size={15} />
</button>

{#if menu}
  <Menu items={items()} x={menu.x} y={menu.y} minWidth={menu.width} onclose={() => (menu = null)} />
{/if}

<style>
  .select {
    display: inline-flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    flex-shrink: 0;
    min-width: 140px;
    max-width: 100%;
    height: 32px;
    padding: 0 10px 0 12px;
    border-radius: var(--radius-sm);
    border: 1px solid var(--border-strong);
    background: var(--surface);
    transition: background 0.15s, border-color 0.15s;
  }
  .select:hover {
    background: var(--surface-hover);
  }
  .select.open {
    border-color: var(--accent);
  }
  .select :global(svg) {
    flex-shrink: 0;
    color: var(--text-2);
    transition: transform 0.15s;
  }
  .select.open :global(svg) {
    transform: rotate(180deg);
  }
</style>
