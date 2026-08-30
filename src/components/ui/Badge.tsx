import type { ReactNode } from "react";
import "./Badge.css";

type Tone = "neutral" | "success" | "danger" | "warning";

interface BadgeProps {
  tone?: Tone;
  children: ReactNode;
}

export function Badge({ tone = "neutral", children }: BadgeProps) {
  return <span className={`badge badge-${tone}`}>{children}</span>;
}
