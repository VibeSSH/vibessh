import { useTranslation } from "react-i18next";
import type { Blueprint } from "@/types/application";

/**
 * A blueprint's name and description in the reader's language.
 *
 * Both strings are authored in Rust, where a blueprint is defined, and a
 * `Blueprint` crosses the bridge carrying the English ones. That made every
 * application type - the grid in the wizard, its tooltip, the description
 * under it, the type picker - English in the middle of an otherwise Polish
 * screen, which is exactly the text somebody reads while deciding what to
 * install.
 *
 * Translating here rather than in Rust because this is the only side that
 * knows which language is being read. The Rust string stays and is the
 * fallback, so a blueprint added without a translation shows its English
 * description rather than a missing-key placeholder - and one added without
 * a *name* translation shows "PostgreSQL", which is the right answer anyway:
 * most of these are product names and translating them would be wrong.
 */
export function useBlueprintText() {
  const { t } = useTranslation();
  return {
    name: (blueprint: Blueprint) => t(`blueprints.${blueprint.id}.name`, { defaultValue: blueprint.name }),
    description: (blueprint: Blueprint) => t(`blueprints.${blueprint.id}.description`, { defaultValue: blueprint.description }),
  };
}
