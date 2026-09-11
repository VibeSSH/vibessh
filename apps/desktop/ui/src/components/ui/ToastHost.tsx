import { useTranslation } from "react-i18next";
import { useToastStore } from "@/stores/toastStore";
import { Icon } from "./Icon";
import "./ToastHost.css";

const TONE_ICON: Record<string, string> = {
  success: "check",
  error: "x",
  info: "activity",
};

/** Mounted once in AppLayout - every page reaches it through the toastSuccess/toastError helpers, not this component directly. */
export function ToastHost() {
  const { t } = useTranslation();
  const toasts = useToastStore((s) => s.toasts);
  const dismiss = useToastStore((s) => s.dismiss);

  if (toasts.length === 0) return null;

  return (
    <div className="toast-host">
      {toasts.map((toast) => (
        <div key={toast.id} className={`toast toast-${toast.tone}`} role="status">
          <Icon name={TONE_ICON[toast.tone]} size={16} />
          <span className="toast-message">{toast.message}</span>
          <button className="toast-dismiss" onClick={() => dismiss(toast.id)} aria-label={t("toast.dismissAria")}>
            <Icon name="x" size={12} />
          </button>
        </div>
      ))}
    </div>
  );
}
