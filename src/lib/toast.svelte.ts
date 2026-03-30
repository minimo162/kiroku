export type ToastType = "success" | "error" | "info";

export type Toast = {
  id: number;
  type: ToastType;
  message: string;
};

let nextToastId = 1;
const dismissTimers = new Map<number, number>();

export const toastState = $state({
  toasts: [] as Toast[]
});

export function addToast(type: ToastType, message: string) {
  const id = nextToastId++;
  toastState.toasts = [...toastState.toasts, { id, type, message }];

  if (type === "success" && typeof window !== "undefined") {
    const timer = window.setTimeout(() => {
      removeToast(id);
    }, 3000);
    dismissTimers.set(id, timer);
  }

  return id;
}

export function removeToast(id: number) {
  const timer = dismissTimers.get(id);
  if (timer !== undefined && typeof window !== "undefined") {
    window.clearTimeout(timer);
    dismissTimers.delete(id);
  }

  toastState.toasts = toastState.toasts.filter((toast) => toast.id !== id);
}
