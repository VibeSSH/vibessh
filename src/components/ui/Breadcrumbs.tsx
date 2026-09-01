import { useEffect, useState } from "react";
import "./Breadcrumbs.css";

interface BreadcrumbsProps {
  segments: string[];
  onNavigate: (path: string) => void;
  /** What `onNavigate` receives for the root itself - matches whatever the
   * caller's own "root path" sentinel is (both current callers use `"."`). */
  rootPath: string;
}

const COLLAPSE_THRESHOLD = 4;
const TAIL_LENGTH = 2;

/** A breadcrumb trail that collapses its middle segments instead of
 * growing forever - past `COLLAPSE_THRESHOLD` segments, shows root, a
 * clickable "…" that reveals the rest, then the last two segments. Shared
 * by the plain Files browser and the per-Application file manager, which
 * previously duplicated this exact rendering. */
export function Breadcrumbs({ segments, onNavigate, rootPath }: BreadcrumbsProps) {
  const [expanded, setExpanded] = useState(false);

  useEffect(() => {
    setExpanded(false);
  }, [segments.join("/")]);

  const shouldCollapse = !expanded && segments.length > COLLAPSE_THRESHOLD;
  const visible = shouldCollapse ? segments.slice(-TAIL_LENGTH) : segments;
  const hiddenCount = segments.length - visible.length;

  return (
    <div className="files-breadcrumb">
      <button className="files-breadcrumb-item" onClick={() => onNavigate(rootPath)}>
        /
      </button>
      {hiddenCount > 0 && (
        <span>
          <span className="files-breadcrumb-sep">/</span>
          <button className="files-breadcrumb-item" onClick={() => setExpanded(true)} title={`${hiddenCount}`}>
            …
          </button>
        </span>
      )}
      {visible.map((segment, i) => {
        const fullIndex = segments.length - visible.length + i;
        const target = segments.slice(0, fullIndex + 1).join("/");
        // The root button above already renders its own trailing "/" - only
        // segments after the first one (or after a "…" collapse marker) need
        // their own separator, otherwise root+segment renders as "//segment".
        const showSep = i > 0 || hiddenCount > 0;
        return (
          <span key={target}>
            {showSep && <span className="files-breadcrumb-sep">/</span>}
            <button className="files-breadcrumb-item" onClick={() => onNavigate(target)}>
              {segment}
            </button>
          </span>
        );
      })}
    </div>
  );
}
