export type ToastKind = 'info' | 'success' | 'error';

export interface Toast {
  id: number;
  kind: ToastKind;
  message: string;
}

const TOAST_TTL_MS = 6000;

/** Transient user notifications (errors, confirmations). */
class ToastStore {
  toasts = $state<Toast[]>([]);
  #next = 1;

  push(kind: ToastKind, message: string): number {
    const id = this.#next++;
    this.toasts = [...this.toasts, { id, kind, message }];
    setTimeout(() => this.dismiss(id), TOAST_TTL_MS);
    return id;
  }

  error(message: string): number {
    return this.push('error', message);
  }

  success(message: string): number {
    return this.push('success', message);
  }

  info(message: string): number {
    return this.push('info', message);
  }

  dismiss(id: number): void {
    this.toasts = this.toasts.filter((toast) => toast.id !== id);
  }
}

export const toasts = new ToastStore();
