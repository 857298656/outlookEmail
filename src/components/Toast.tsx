import { CheckCircle2 } from "lucide-react";

export type ToastMessage = {
  id: number;
  message: string;
};

export function Toast({ toast }: { toast: ToastMessage | null }) {
  if (!toast) return null;

  return (
    <div className="toastNotice" role="status" aria-live="polite">
      <CheckCircle2 size={16} />
      <span>{toast.message}</span>
    </div>
  );
}
