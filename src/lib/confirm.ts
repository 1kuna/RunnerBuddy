import { get, writable } from "svelte/store";

export type ConfirmDialogConfig = {
  title?: string;
  message: string;
  expected?: string | null;
  confirmText?: string;
  cancelText?: string;
  resolve: (confirmed: boolean) => void;
};

export const confirmDialog = writable<ConfirmDialogConfig | null>(null);

export async function confirmAction(options: {
  title?: string;
  message: string;
  expected?: string | null;
  confirmText?: string;
  cancelText?: string;
}): Promise<boolean> {
  return new Promise((resolve) => {
    confirmDialog.set({
      title: options.title,
      message: options.message,
      expected: options.expected ?? null,
      confirmText: options.confirmText,
      cancelText: options.cancelText,
      resolve,
    });
  });
}

export function closeConfirmDialog(confirmed: boolean) {
  const dialog = get(confirmDialog);
  confirmDialog.set(null);
  dialog?.resolve(confirmed);
}
