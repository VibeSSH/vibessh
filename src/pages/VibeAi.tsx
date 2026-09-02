import { useEffect, useRef, useState, type FormEvent, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { Link } from "react-router-dom";
import { Badge } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { EmptyState } from "@/components/ui/EmptyState";
import { Icon } from "@/components/ui/Icon";
import { getAiConfig, previewAiContext } from "@/services/aiService";
import { errorMessage } from "@/services/tauri";
import { useAiStore } from "@/stores/aiStore";
import type { AiConfigView, AiContextBundle, AiMode } from "@/types/ai";
import "./pages.css";
import "./VibeAi.css";

const MODES: AiMode[] = ["ask", "diagnose"];

/**
 * The assistant panel.
 *
 * Two things here are not decoration.
 *
 * The **context preview** is a disclosure, not a nicety: `AGENTS.md` §6 says
 * a boundary the interface does not show is not a boundary, and "which facts
 * about my server leave this machine" is exactly such a boundary. It shows
 * the literal text that will be sent, fetched from the same builder the turn
 * uses, so the two cannot drift.
 *
 * The **not-configured state** links straight to Settings rather than
 * showing an input that would fail on submit. Diagnosing a broken endpoint
 * from an error banner is worse than never offering the box.
 */
export function VibeAi() {
  const { t } = useTranslation();
  const { messages, mode, context, contextLabel, turnId, error, phase, setMode, clearConversation, send, stop, consumePendingQuestion } =
    useAiStore();

  const [config, setConfig] = useState<AiConfigView | null>(null);
  const [loading, setLoading] = useState(true);
  const [draft, setDraft] = useState("");
  const [preview, setPreview] = useState<AiContextBundle | null>(null);
  const [previewOpen, setPreviewOpen] = useState(false);
  const [previewLoading, setPreviewLoading] = useState(false);

  const transcriptRef = useRef<HTMLDivElement>(null);
  const busy = turnId !== null;

  useEffect(() => {
    getAiConfig()
      .then(setConfig)
      .catch(() => setConfig(null))
      .finally(() => setLoading(false));
  }, []);

  // A quick action ("Ask Vibe AI" on a failed Application) seeds a question
  // and navigates here; this is where it actually gets sent, once, after the
  // store has the context it was seeded with.
  useEffect(() => {
    const seeded = consumePendingQuestion();
    if (seeded) void send(seeded);
  }, [consumePendingQuestion, send]);

  useEffect(() => {
    const node = transcriptRef.current;
    if (node) node.scrollTop = node.scrollHeight;
  }, [messages]);

  async function togglePreview() {
    if (previewOpen) {
      setPreviewOpen(false);
      return;
    }
    setPreviewOpen(true);
    setPreviewLoading(true);
    try {
      setPreview(await previewAiContext(mode, context));
    } catch {
      // The preview is an extra; a failure to build one must not become an
      // error banner over the conversation. The panel shows "nothing to
      // preview", which is true from the user's point of view.
      setPreview(null);
    } finally {
      setPreviewLoading(false);
    }
  }

  function handleSubmit(event: FormEvent) {
    event.preventDefault();
    if (busy || !draft.trim()) return;
    const question = draft;
    setDraft("");
    void send(question);
  }

  function handleKeyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
    // Enter sends, Shift+Enter breaks the line - what every chat box does,
    // and worth matching because the alternative surprises people into
    // sending half a question.
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      handleSubmit(event as unknown as FormEvent);
    }
  }

  if (loading) return null;

  const ready = config?.enabled && config.baseUrl && config.model;

  return (
    <div className="page vibe-ai-page">
      <div className="page-header">
        <h1 className="page-title">{t("vibeAi.title")}</h1>
        <p className="page-subtitle">{t("vibeAi.subtitle")}</p>
      </div>

      {!ready ? (
        <EmptyState
          icon="sparkles"
          title={t("vibeAi.notConfiguredTitle")}
          description={t("vibeAi.notConfiguredDescription")}
          action={
            <Link to="/settings" className="btn btn-primary btn-md">
              <Icon name="settings" size={14} />
              {t("vibeAi.openSettings")}
            </Link>
          }
        />
      ) : (
        <div className="vibe-ai-shell">
          <div className="vibe-ai-toolbar">
            <div className="vibe-ai-modes" role="group" aria-label={t("vibeAi.modeAria")}>
              {MODES.map((option) => (
                <button
                  key={option}
                  type="button"
                  className={`vibe-ai-mode ${mode === option ? "vibe-ai-mode-active" : ""}`}
                  onClick={() => setMode(option)}
                  aria-pressed={mode === option}
                  title={t(`vibeAi.mode_${option}_hint`)}
                >
                  {t(`vibeAi.mode_${option}`)}
                </button>
              ))}
            </div>

            <div className="vibe-ai-toolbar-right">
              {mode === "diagnose" &&
                (context ? (
                  <Badge tone="neutral">{contextLabel ?? t("vibeAi.contextUnnamed")}</Badge>
                ) : (
                  <Badge tone="warning">{t("vibeAi.noContext")}</Badge>
                ))}
              {mode === "diagnose" && context && (
                <Button variant="secondary" size="sm" onClick={() => void togglePreview()} aria-expanded={previewOpen}>
                  <Icon name="eye" size={14} />
                  {t("vibeAi.previewContext")}
                </Button>
              )}
              <Button variant="secondary" size="sm" onClick={clearConversation} disabled={messages.length === 0}>
                <Icon name="trash" size={14} />
                {t("vibeAi.clear")}
              </Button>
            </div>
          </div>

          {previewOpen && (
            <div className="vibe-ai-preview">
              <p className="vibe-ai-preview-title">{t("vibeAi.previewTitle")}</p>
              {previewLoading ? (
                <p className="vibe-ai-preview-empty">{t("vibeAi.previewLoading")}</p>
              ) : preview ? (
                <>
                  {preview.notes.length > 0 && (
                    <ul className="vibe-ai-preview-notes">
                      {preview.notes.map((note) => (
                        <li key={note}>{note}</li>
                      ))}
                    </ul>
                  )}
                  <pre className="vibe-ai-preview-body">{preview.summary}</pre>
                </>
              ) : (
                <p className="vibe-ai-preview-empty">{t("vibeAi.previewEmpty")}</p>
              )}
            </div>
          )}

          <div className="vibe-ai-transcript" ref={transcriptRef}>
            {messages.length === 0 ? (
              <div className="vibe-ai-intro">
                <Icon name="sparkles" size={22} />
                <p className="vibe-ai-intro-title">{t("vibeAi.introTitle")}</p>
                <p className="vibe-ai-intro-text">{t("vibeAi.introText")}</p>
              </div>
            ) : (
              messages.map((message) => (
                <div key={message.id} className={`vibe-ai-message vibe-ai-message-${message.role}`}>
                  <div className="vibe-ai-bubble">
                    {message.content}
                    {message.pending && <span className="vibe-ai-caret" aria-hidden="true" />}
                    {message.pending && message.content === "" && (
                      <span className="vibe-ai-thinking">{phase === "collecting" ? t("vibeAi.collecting") : t("vibeAi.thinking")}</span>
                    )}
                  </div>
                  {message.stopped && <p className="vibe-ai-stopped">{t("vibeAi.stoppedNote")}</p>}
                </div>
              ))
            )}
          </div>

          {error != null && (
            <p className="form-note form-note-danger form-note-spaced" role="alert">
              {errorMessage(error, t)}
            </p>
          )}

          <form className="vibe-ai-composer" onSubmit={handleSubmit}>
            <textarea
              className="vibe-ai-input"
              value={draft}
              onChange={(event) => setDraft(event.target.value)}
              onKeyDown={handleKeyDown}
              placeholder={t("vibeAi.placeholder")}
              rows={3}
              aria-label={t("vibeAi.inputAria")}
            />
            {busy ? (
              <Button type="button" variant="secondary" onClick={() => void stop()}>
                <Icon name="square" size={14} />
                {t("vibeAi.stop")}
              </Button>
            ) : (
              <Button type="submit" variant="primary" disabled={!draft.trim()}>
                <Icon name="send" size={14} />
                {t("vibeAi.send")}
              </Button>
            )}
          </form>

          <p className="vibe-ai-disclaimer">{t("vibeAi.disclaimer")}</p>
        </div>
      )}
    </div>
  );
}
