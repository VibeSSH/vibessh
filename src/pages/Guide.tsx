import { useDeferredValue, useMemo, useState } from "react";
import { useNavigate, useSearchParams } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { EmptyState } from "@/components/ui/EmptyState";
import { Icon } from "@/components/ui/Icon";
import { Markdown } from "@/guide/Markdown";
import { guideDocs, searchGuide, type GuideDoc } from "@/guide/guideDocs";
import { guideImage } from "@/guide/guideImages";
import "./pages.css";
import "./Guide.css";

/**
 * The manual, inside the application it documents.
 *
 * Two things make this worth having rather than a link to a website. It is
 * the same text Vibe AI answers from - one corpus, so the page and the
 * assistant cannot disagree - and every feature can point straight at its
 * own topic, which is what `GuideLink` does. Somebody who does not know what
 * a field means does not have to know what the feature is called first.
 */
export function Guide() {
  const { t, i18n } = useTranslation();
  const navigate = useNavigate();
  const [params, setParams] = useSearchParams();
  const [query, setQuery] = useState("");
  // The list re-renders on every character; the field must not wait for it.
  const deferredQuery = useDeferredValue(query);

  const language = i18n.language;
  const docs = useMemo(() => guideDocs(language), [language]);
  const results = useMemo(() => searchGuide(deferredQuery, language), [deferredQuery, language]);

  // The topic in the URL, so a link from a feature - and the back button -
  // both land where they should.
  const selectedId = params.get("topic") ?? results[0]?.id ?? docs[0]?.id;
  const selected = docs.find((doc) => doc.id === selectedId) ?? results[0] ?? docs[0];

  const sections = useMemo(() => {
    const grouped = new Map<string, GuideDoc[]>();
    for (const doc of results) {
      const list = grouped.get(doc.section) ?? [];
      list.push(doc);
      grouped.set(doc.section, list);
    }
    return [...grouped.entries()];
  }, [results]);

  function select(id: string) {
    // `replace`, so reading through the guide does not fill the history with
    // every topic somebody glanced at on the way.
    setParams({ topic: id }, { replace: true });
  }

  return (
    <div className="page guide-page">
      <div className="page-header">
        <h1 className="page-title">{t("guide.title")}</h1>
        <p className="page-subtitle">{t("guide.subtitle")}</p>
      </div>

      <div className="guide-layout">
        <aside className="guide-sidebar">
          <input
            className="form-input guide-search"
            type="search"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder={t("guide.searchPlaceholder")}
            aria-label={t("guide.searchPlaceholder")}
          />

          {sections.length === 0 ? (
            <p className="form-note guide-no-results">{t("guide.noResults")}</p>
          ) : (
            sections.map(([section, items]) => (
              <div key={section} className="guide-sidebar-section">
                <p className="guide-sidebar-heading">{t(`guide.sections.${section}`, { defaultValue: section })}</p>
                <ul className="guide-sidebar-list">
                  {items.map((doc) => (
                    <li key={doc.id}>
                      <button
                        className={`guide-sidebar-item ${doc.id === selected?.id ? "guide-sidebar-item-active" : ""}`}
                        onClick={() => select(doc.id)}
                      >
                        {doc.title}
                      </button>
                    </li>
                  ))}
                </ul>
              </div>
            ))
          )}
        </aside>

        <Card className="guide-reader">
          {selected ? (
            <>
              <div className="guide-reader-header">
                <div>
                  <h2 className="guide-reader-title">{selected.title}</h2>
                  {/* Said plainly rather than hidden: a topic not yet
                      written in this language is shown in the one it exists
                      in, which is worth more than a gap, but the reader
                      should not have to work out why it changed language. */}
                  {selected.language !== language.split("-")[0] && (
                    <p className="form-note">{t("guide.otherLanguage", { language: selected.language.toUpperCase() })}</p>
                  )}
                </div>
                {selected.route && (
                  <Button variant="secondary" size="sm" onClick={() => navigate(selected.route as string)}>
                    <Icon name="chevron-right" size={14} />
                    {t("guide.openFeature")}
                  </Button>
                )}
              </div>
              <Markdown source={selected.body} resolveImage={guideImage} />
            </>
          ) : (
            <EmptyState icon="file" title={t("guide.emptyTitle")} description={t("guide.emptyDescription")} />
          )}
        </Card>
      </div>
    </div>
  );
}
