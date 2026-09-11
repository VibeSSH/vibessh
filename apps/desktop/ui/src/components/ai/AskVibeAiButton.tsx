import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";
import { Button } from "@/components/ui/Button";
import { Icon } from "@/components/ui/Icon";
import { useAiStore } from "@/stores/aiStore";
import type { AiContextRef } from "@/types/ai";

interface AskVibeAiButtonProps {
  /** What the conversation will be about. Also what gets collected. */
  context: AiContextRef;
  /** The name to show in the panel's context chip - "paper-survival". */
  contextLabel: string;
  /** The question to ask on the user's behalf. A translated sentence, not a
   * prompt fragment: it appears in the transcript as the user's own message,
   * so it has to read like something they would have typed. */
  question: string;
  label?: string;
}

/**
 * "Ask Vibe AI" - the one-click path from a thing that went wrong to a
 * conversation about it.
 *
 * Seeds the store and navigates; the panel sends the seeded question once it
 * mounts. Deliberately not a modal or an inline answer: the user ends up in
 * the same place, with the same history and the same Stop button, as if they
 * had opened the assistant themselves. A second, cut-down chat surface would
 * be a second set of behaviours to get right.
 *
 * This button never appears unless the assistant is configured - see its
 * call sites - because offering help that resolves to "go to Settings" at
 * the moment something is already broken is worse than not offering it.
 */
export function AskVibeAiButton({ context, contextLabel, question, label }: AskVibeAiButtonProps) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const seed = useAiStore((state) => state.seed);

  return (
    <Button
      variant="secondary"
      size="sm"
      onClick={() => {
        seed({ context, contextLabel, question });
        navigate("/vibe-ai");
      }}
    >
      <Icon name="sparkles" size={14} />
      {label ?? t("vibeAi.askAction")}
    </Button>
  );
}
