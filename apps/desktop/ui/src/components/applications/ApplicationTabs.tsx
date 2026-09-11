import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";
import { Icon } from "@/components/ui/Icon";
import { useApplicationTabsStore } from "@/stores/applicationTabsStore";
import "./ApplicationTabs.css";

interface ApplicationTabsProps {
  /** The Application currently on screen. Absent on the list, where none is. */
  activeId?: string;
}

/**
 * The strip of open Applications.
 *
 * **Closing a tab does not touch the Application.** That is the one thing
 * this control has to get right: it sits next to buttons that stop and
 * delete real servers, and an `x` that reads as "remove this" would be a
 * catastrophic misreading. It is only ever a bookmark being dropped, which
 * is why the close control is quiet and why nothing here confirms anything.
 */
export function ApplicationTabs({ activeId }: ApplicationTabsProps) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const tabs = useApplicationTabsStore((state) => state.tabs);
  const close = useApplicationTabsStore((state) => state.close);

  // Shown from the first tab. It was hidden below two on the theory that a
  // single tab only repeats the page you are on - which is true inside an
  // Application and wrong on the list, where one tab is exactly the "take me
  // back to what I was doing" the strip exists for.
  if (tabs.length === 0) return null;

  function handleClose(id: string) {
    close(id);
    // Closing the tab you are on leaves you looking at an Application with
    // no tab. Falling back to the list is the honest place to land.
    if (id === activeId) {
      const remaining = tabs.filter((tab) => tab.id !== id);
      navigate(remaining.length > 0 ? `/applications/${remaining[remaining.length - 1].id}` : "/applications");
    }
  }

  return (
    <div className="application-tabs" role="tablist" aria-label={t("applicationTabs.label")}>
      {tabs.map((tab) => (
        <div key={tab.id} className={`application-tab${tab.id === activeId ? " application-tab-active" : ""}`}>
          <button
            type="button"
            role="tab"
            aria-selected={tab.id === activeId}
            className="application-tab-name"
            title={tab.name}
            onClick={() => navigate(`/applications/${tab.id}`)}
          >
            {tab.name}
          </button>
          <button
            type="button"
            className="application-tab-close"
            title={t("applicationTabs.close", { name: tab.name })}
            aria-label={t("applicationTabs.close", { name: tab.name })}
            onClick={() => handleClose(tab.id)}
          >
            <Icon name="x" size={12} />
          </button>
        </div>
      ))}
    </div>
  );
}
