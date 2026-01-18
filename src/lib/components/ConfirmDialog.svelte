<script lang="ts">
  type Props = {
    open: boolean;
    title?: string;
    message: string;
    expected?: string | null;
    confirmText?: string;
    cancelText?: string;
    busy?: boolean;
    onConfirm: () => void;
    onCancel: () => void;
  };

  let {
    open,
    title,
    message,
    expected = null,
    confirmText = "Confirm",
    cancelText = "Cancel",
    busy = false,
    onConfirm,
    onCancel,
  }: Props = $props();

  let input = $state("");

  $effect(() => {
    if (open) {
      input = "";
    }
  });

  const canConfirm = $derived(() => {
    if (busy) return false;
    if (!expected) return true;
    return input === expected;
  });
</script>

{#if open}
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-slate-950/70 p-6">
    <div class="w-full max-w-lg rounded-2xl border border-slate-500/40 bg-slate-950/80 px-6 py-5 glass-panel">
      {#if title}
        <h2 class="text-lg font-semibold text-white">{title}</h2>
      {/if}
      <p class="mt-2 whitespace-pre-wrap text-sm text-slate-200">{message}</p>
      {#if expected}
        <p class="mt-3 text-xs text-slate-400">Type “{expected}” to confirm.</p>
        <input
          class="mt-2 w-full rounded-xl border border-slate-500/40 bg-slate-950/40 px-4 py-2 text-sm text-white"
          placeholder={expected}
          bind:value={input}
        />
      {/if}
      <div class="mt-4 flex flex-wrap justify-end gap-3">
        <button
          class="rounded-xl border border-slate-500/40 px-4 py-2 text-sm font-semibold text-slate-200"
          onclick={onCancel}
          disabled={busy}
        >
          {cancelText}
        </button>
        <button
          class="rounded-xl bg-tide-500 px-4 py-2 text-sm font-semibold text-white disabled:cursor-not-allowed disabled:opacity-60"
          onclick={onConfirm}
          disabled={!canConfirm}
        >
          {confirmText}
        </button>
      </div>
    </div>
  </div>
{/if}
