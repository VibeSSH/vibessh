import { useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { IconButton } from "@/components/ui/IconButton";
import { guideDoc } from "./guideDocs";

interface GuideLinkProps {
  /** The topic's id - its file name in `shared/guide`. */
  topic: string;
}

/**
 * "What is this?", next to the thing being asked about.
 *
 * A manual nobody can find is a manual nobody reads. This puts the relevant
 * page one click from the feature itself, so somebody looking at a field
 * they do not understand does not first have to work out what the feature
 * is called in order to search for it.
 *
 * Renders nothing when the topic does not exist. A guide is written over
 * time and a button that leads to an empty page is worse than no button -
 * and this way a link placed before its topic is written simply appears
 * when the topic does.
 */
export function GuideLink({ topic }: GuideLinkProps) {
  const { t, i18n } = useTranslation();
  const navigate = useNavigate();
  if (!guideDoc(topic, i18n.language)) return null;

  return (
    <IconButton
      icon="help-circle"
      size="sm"
      title={t("guide.linkAria")}
      onClick={() => navigate(`/guide?topic=${encodeURIComponent(topic)}`)}
    />
  );
}
