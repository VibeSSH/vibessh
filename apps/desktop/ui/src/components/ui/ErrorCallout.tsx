import { useTranslation } from "react-i18next";
import { Icon } from "@/components/ui/Icon";
import { AskVibeAiButton } from "@/components/ai/AskVibeAiButton";
import type { AiContextRef } from "@/types/ai";
import "./ErrorCallout.css";

interface ErrorCalloutProps {
  /** The already-translated sentence to show - build it with `errorMessage`. */
  message: string;
  /** When given, adds a one-click "Ask Vibe AI about this" button seeded with
   *  the error, so a failure that is hard to read is one click from an
   *  explanation. Shown even when the assistant is not configured yet - a
   *  stuck user is exactly who wants that path, and the button then leads to
   *  turning it on rather than to a dead end. */
  ai?: { context: AiContextRef; contextLabel: string };
  className?: string;
}

/**
 * A failure, shown as a calm bordered callout rather than a raw red sentence.
 *
 * Every form and dialog used to drop the backend's error text into a bare
 * `.form-note-danger` paragraph - a wall of red, often a raw technical
 * sentence, with nothing to do about it. This gives that text a consistent
 * frame (a warning glyph, a tinted panel, room to breathe) and, where a
 * context is known, the "Ask Vibe AI" path straight from the thing that broke
 * to a conversation about it.
 */
export function ErrorCallout({ message, ai, className }: ErrorCalloutProps) {
  const { t } = useTranslation();
  return (
    <div className={`error-callout ${className ?? ""}`.trim()} role="alert">
      <Icon name="alert-triangle" size={16} className="error-callout-icon" />
      <div className="error-callout-body">
        <p className="error-callout-message">{message}</p>
        {ai && (
          <div className="error-callout-actions">
            <AskVibeAiButton
              context={ai.context}
              contextLabel={ai.contextLabel}
              question={t("vibeAi.seedError", { error: message })}
              label={t("vibeAi.askAboutError")}
            />
          </div>
        )}
      </div>
    </div>
  );
}
