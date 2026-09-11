import { useEffect, useMemo, useRef, useState, type FormEvent, type KeyboardEvent } from "react";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { Link } from "react-router-dom";
import { open } from "@tauri-apps/plugin-shell";
import { Markdown } from "@/guide/Markdown";
import { AiUsageModal } from "@/components/ai/AiUsageModal";
import { Button } from "@/components/ui/Button";
import { EmptyState } from "@/components/ui/EmptyState";
import { Icon } from "@/components/ui/Icon";
import { Select, type SelectItem } from "@/components/ui/Select";
import { getAiConfig, getAiQuota, previewAiContext } from "@/services/aiService";
import { listApplications } from "@/services/applicationService";
import { queryKeys } from "@/services/queryKeys";
import { listServers } from "@/services/serverService";
import { errorMessage } from "@/services/tauri";
import { useAiStore } from "@/stores/aiStore";
import type { AiConfigView, AiContextBundle, AiMode, AiQuota } from "@/types/ai";
import "./pages.css";
import "./VibeAi.css";

const MODES: AiMode[] = ["ask", "diagnose"];

/**
 * A link in an answer goes to the system browser, never to this webview.
 * The app has no address bar and no back button, so following one in place
 * would strand somebody on a page written by the model.
 */
function openExternally(href: string) {
  if (!/^https?:\/\//i.test(href)) return;
  open(href).catch(() => {
    // Nothing useful to say if the desktop refuses to open a browser, and
    // the answer itself is still on screen with the address in it.
  });
}

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
  const { messages, mode, context, contextLabel, turnId, error, phase, setMode, setContext, clearConversation, send, stop, consumePendingQuestion } =
    useAiStore();

  const [config, setConfig] = useState<AiConfigView | null>(null);
  const [loading, setLoading] = useState(true);
  const [draft, setDraft] = useState("");
  const [preview, setPreview] = useState<AiContextBundle | null>(null);
  const [previewOpen, setPreviewOpen] = useState(false);
  const [previewLoading, setPreviewLoading] = useState(false);
  const [quota, setQuota] = useState<AiQuota | null>(null);
  const [usageOpen, setUsageOpen] = useState(false);

  // What a Diagnose turn can be about. Fetched here rather than read from the
  // servers store, which is filled by the pages that list servers - opening
  // this panel straight from the sidebar would otherwise offer an empty list.
  const { data: servers = [] } = useQuery({ queryKey: queryKeys.servers(), queryFn: listServers, staleTime: 30_000, retry: false });
  const { data: applications = [] } = useQuery({
    queryKey: queryKeys.applications(),
    queryFn: listApplications,
    staleTime: 30_000,
    retry: false,
  });

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

  // Re-read the allowance whenever a turn ends rather than on a timer:
  // finishing a question is the only thing that changes it from here,
  // and polling an endpoint to watch a number the user is not looking
  // at would be work for nothing. Null means this install uses its own
  // API key and has no allowance to show.
  useEffect(() => {
    if (turnId !== null) return;
    getAiQuota()
      .then(setQuota)
      .catch(() => setQuota(null));
  }, [turnId]);

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

  /// The subjects a Diagnose turn can be about, grouped the way the sidebar
  /// groups them. A context seeded by a quick action is listed even when it
  /// is in neither list - the lists may not have loaded, and without its own
  /// entry the picker would read as empty while a subject was in fact
  /// attached to the turn.
  const subjectItems = useMemo<SelectItem[]>(() => {
    const items: SelectItem[] = [{ value: "", label: t("vibeAi.subjectNone") }];
    const selected = context ? `${context.kind}:${context.id}` : "";
    const listed =
      servers.some((server) => `node:${server.id}` === selected) ||
      applications.some((application) => `application:${application.id}` === selected);
    if (context && !listed) {
      items.push({ value: selected, label: contextLabel ?? t("vibeAi.contextUnnamed") });
    }
    if (servers.length > 0) {
      items.push({
        label: t("vibeAi.subjectNodes"),
        options: servers.map((server) => ({ value: `node:${server.id}`, label: server.name })),
      });
    }
    if (applications.length > 0) {
      items.push({
        label: t("vibeAi.subjectApplications"),
        options: applications.map((application) => ({ value: `application:${application.id}`, label: application.name })),
      });
    }
    return items;
  }, [applications, context, contextLabel, servers, t]);

  /// The subject is encoded as "kind:id" because the picker carries one
  /// string, and both halves are needed to build the reference.
  function handleSubjectChange(value: string) {
    if (value === "") {
      setContext(null, null);
      return;
    }
    const separator = value.indexOf(":");
    const kind = value.slice(0, separator);
    const id = value.slice(separator + 1);
    if (kind === "node") {
      setContext({ kind: "node", id }, servers.find((server) => server.id === id)?.name ?? id);
    } else {
      setContext({ kind: "application", id }, applications.find((application) => application.id === id)?.name ?? id);
    }
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

  // The included model carries no base URL and no model name - both belong to
  // the bring-your-own-endpoint path and live on the backend for the hosted
  // one. Requiring them of every provider made the one provider that needs no
  // configuring the only one that could never look configured.
  const ready = config?.enabled && (config.provider === "vibeSshHosted" || (config.baseUrl !== "" && config.model !== ""));

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
              {quota && (
                <Button
                  variant="secondary"
                  size="sm"
                  onClick={() => setUsageOpen(true)}
                  title={t("aiUsage.title")}
                >
                  <Icon name="activity" size={14} />
                  {t("aiUsage.chip", { remaining: Math.max(quota.limit - quota.used, 0), limit: quota.limit })}
                </Button>
              )}
              {/* A warning badge saying "nothing selected" was a dead end:
                * it named the problem and offered no way out, and the only
                * cure was to leave, find the Node and come back through a
                * quick action. The same spot now picks the subject. */}
              {mode === "diagnose" && (
                <Select
                  className={`vibe-ai-subject ${context ? "" : "vibe-ai-subject-empty"}`.trim()}
                  value={context ? `${context.kind}:${context.id}` : ""}
                  onChange={handleSubjectChange}
                  aria-label={t("vibeAi.subjectLabel")}
                  items={subjectItems}
                />
              )}
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
                    {/* Only the assistant's half is markdown. What somebody
                      * typed is shown as they typed it - reading their own
                      * asterisks back as bold would be the panel editing the
                      * question. */}
                    {message.role === "assistant" && message.content !== "" ? (
                      <Markdown source={message.content} onLinkClick={openExternally} />
                    ) : (
                      message.content
                    )}
                    {message.pending && message.content !== "" && <span className="vibe-ai-caret" aria-hidden="true" />}
                    {message.pending && message.content === "" && (
                      <span
                        className="vibe-ai-pending"
                        role="status"
                        aria-label={phase === "collecting" ? t("vibeAi.collecting") : t("vibeAi.thinking")}
                      >
                        {/* The collecting phase keeps its words: a Node that never answers
                          * is worth naming, and dots alone would hide it behind what looks
                          * like a slow model. Waiting on the model is just the dots. */}
                        {phase === "collecting" && (
                          <span className="vibe-ai-thinking" aria-hidden="true">{t("vibeAi.collecting")}</span>
                        )}
                        <span className="vibe-ai-dots" aria-hidden="true">
                          <span />
                          <span />
                          <span />
                        </span>
                      </span>
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

      {usageOpen && quota && <AiUsageModal quota={quota} onClose={() => setUsageOpen(false)} />}
    </div>
  );
}
