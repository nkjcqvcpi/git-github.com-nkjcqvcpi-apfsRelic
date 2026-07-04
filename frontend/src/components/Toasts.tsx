import { ToastNotification } from "@carbon/react";

export type ToastKind = "success" | "error" | "info" | "warning";

export interface ToastItem {
  id: number;
  kind: ToastKind;
  title: string;
  subtitle?: string;
}

interface ToastsProps {
  toasts: ToastItem[];
  onDismiss: (id: number) => void;
}

/** Bottom-right stack of auto-dismissing Carbon toasts. */
export default function Toasts({ toasts, onDismiss }: ToastsProps) {
  if (toasts.length === 0) return null;
  return (
    <div className="toast-stack" role="status" aria-live="polite">
      {toasts.map((t) => (
        <ToastNotification
          key={t.id}
          kind={t.kind}
          title={t.title}
          subtitle={t.subtitle}
          lowContrast
          timeout={t.kind === "error" ? 8000 : 4500}
          onClose={() => {
            onDismiss(t.id);
            // Returning true lets Carbon run its close animation.
            return true;
          }}
        />
      ))}
    </div>
  );
}
