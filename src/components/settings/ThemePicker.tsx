import { useState } from "react";
import { useTranslation } from "react-i18next";
import { copyToClipboard } from "@/utils/copyToClipboard";
import { Button } from "@/components/ui/Button";
import { applyTheme, clearTheme, parseColor, parseTheme } from "@/theme/applyTheme";
import { allThemes, loadSelectedThemeId, resolveTheme, saveCustomTheme, saveSelectedThemeId } from "@/theme/themeStore";
import { DEFAULT_THEME_ID } from "@/theme/themes";
import { THEMABLE_TOKENS, type ThemableToken, type Theme } from "@/theme/tokens";
import "./ThemePicker.css";

/** The colours worth putting in front of somebody, in the order they read. */
const EDITABLE_GROUPS: { labelKey: string; tokens: ThemableToken[] }[] = [
  { labelKey: "theme.groupSurfaces", tokens: ["--surface-bg", "--surface-0", "--surface-1", "--surface-2", "--surface-3"] },
  { labelKey: "theme.groupText", tokens: ["--text-primary", "--text-secondary", "--text-tertiary"] },
  { labelKey: "theme.groupAccent", tokens: ["--accent", "--accent-hover", "--border", "--border-hover"] },
  { labelKey: "theme.groupStatus", tokens: ["--success", "--warning", "--danger", "--danger-hover"] },
];

/**
 * Choosing and editing the palette.
 *
 * A theme here is sixteen colours and nothing else - see `theme/tokens.ts`
 * for why it is a closed list rather than a stylesheet. Everything else the
 * interface draws (elevation, rings, hover surfaces, the contrast of a label
 * against its own button) is computed from those.
 */
export function ThemePicker() {
  const { t } = useTranslation();
  const [selected, setSelected] = useState(loadSelectedThemeId);
  const [themes, setThemes] = useState(allThemes);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState<Record<ThemableToken, string>>(() => {
    const base = resolveTheme(loadSelectedThemeId()) ?? themes[0];
    return { ...base.colors };
  });
  const [importError, setImportError] = useState<string | null>(null);

  function choose(id: string) {
    setSelected(id);
    saveSelectedThemeId(id);
    if (id === DEFAULT_THEME_ID) clearTheme();
    else {
      const theme = resolveTheme(id);
      if (theme) applyTheme(theme);
    }
  }

  /** Live preview: every nudge of a colour repaints the whole window. */
  function editColor(token: ThemableToken, value: string) {
    const next = { ...draft, [token]: value };
    setDraft(next);
    applyTheme({ id: "custom", name: t("theme.customName"), colors: next });
  }

  function saveCustom() {
    const theme: Theme = { id: "custom", name: t("theme.customName"), colors: draft };
    saveCustomTheme(theme);
    saveSelectedThemeId("custom");
    setSelected("custom");
    setThemes(allThemes());
    setEditing(false);
  }

  function cancelEditing() {
    setEditing(false);
    choose(selected);
  }

  function exportTheme() {
    const theme = selected === "custom" ? { id: "custom", name: t("theme.customName"), colors: draft } : resolveTheme(selected);
    if (!theme) return;
    void copyToClipboard(JSON.stringify(theme, null, 2), { copied: t("common.copied"), failed: t("common.copyFailed") });
  }

  async function importTheme() {
    setImportError(null);
    try {
      const parsed = parseTheme(JSON.parse(await navigator.clipboard.readText()));
      if (!parsed) {
        setImportError(t("theme.importInvalid"));
        return;
      }
      const theme: Theme = { ...parsed, id: "custom" };
      saveCustomTheme(theme);
      setDraft({ ...theme.colors });
      setThemes(allThemes());
      choose("custom");
    } catch {
      setImportError(t("theme.importInvalid"));
    }
  }

  return (
    <div className="theme-picker">
      <div className="theme-list">
        {themes.map((theme) => (
          <button
            key={theme.id}
            className={`theme-swatch-card ${selected === theme.id ? "theme-swatch-card-active" : ""}`}
            onClick={() => choose(theme.id)}
            aria-pressed={selected === theme.id}
          >
            <span className="theme-swatch-row" aria-hidden="true">
              {(["--surface-1", "--accent", "--success", "--warning", "--danger"] as ThemableToken[]).map((token) => (
                <span key={token} className="theme-swatch" style={{ background: theme.colors[token] }} />
              ))}
            </span>
            <span className="theme-swatch-name">{theme.name}</span>
            {theme.credit && <span className="theme-swatch-credit">{theme.credit}</span>}
          </button>
        ))}
      </div>

      <div className="theme-actions">
        <Button variant="secondary" size="sm" onClick={() => setEditing((open) => !open)}>
          {editing ? t("theme.editClose") : t("theme.edit")}
        </Button>
        <Button variant="secondary" size="sm" onClick={exportTheme}>
          {t("theme.copy")}
        </Button>
        <Button variant="secondary" size="sm" onClick={() => void importTheme()}>
          {t("theme.paste")}
        </Button>
      </div>

      {importError && <p className="form-note form-note-danger">{importError}</p>}

      {editing && (
        <div className="theme-editor">
          <p className="form-note">{t("theme.editorNote")}</p>
          {EDITABLE_GROUPS.map((group) => (
            <div key={group.labelKey} className="theme-editor-group">
              <p className="theme-editor-group-label">{t(group.labelKey)}</p>
              <div className="theme-editor-swatches">
                {group.tokens.map((token) => (
                  <label key={token} className="theme-editor-field">
                    <input
                      type="color"
                      value={parseColor(draft[token])?.slice(0, 7) ?? "#000000"}
                      onChange={(event) => editColor(token, event.target.value)}
                      aria-label={token}
                    />
                    <span className="theme-editor-token">{token.replace(/^--/, "")}</span>
                  </label>
                ))}
              </div>
            </div>
          ))}
          <div className="form-actions">
            <Button variant="secondary" onClick={cancelEditing}>
              {t("common.cancel")}
            </Button>
            <Button onClick={saveCustom}>{t("theme.save")}</Button>
          </div>
        </div>
      )}

      {/* The remaining themable tokens are edited too, just not shown as their
          own swatch - they carry over from whichever theme the draft started
          from. */}
      <p className="settings-muted">{t("theme.tokenCount", { count: THEMABLE_TOKENS.length })}</p>
    </div>
  );
}
