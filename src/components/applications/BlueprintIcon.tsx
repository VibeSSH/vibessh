import { Icon } from "@/components/ui/Icon";
import { blueprintIcon } from "./blueprintIcons";
import "./BlueprintIcon.css";

interface BlueprintIconProps {
  blueprintId: string | undefined;
  size?: number;
}

/**
 * The icon for an Application's blueprint, or the generic glyph when there
 * isn't one.
 *
 * Monochrome paths inherit `currentColor`, so they take the surrounding
 * theme's colour and stay legible in either palette. Lettermarks carry their
 * own tint on purpose - four Minecraft server projects rendered in one accent
 * would be exactly as indistinguishable as the boxes they replaced, which is
 * the problem this solves.
 */
export function BlueprintIcon({ blueprintId, size = 16 }: BlueprintIconProps) {
  const icon = blueprintIcon(blueprintId);

  if (!icon) {
    return <Icon name="box" size={size} />;
  }

  if (icon.kind === "image") {
    // `<img>`, not inlined markup: a browser does not run script from an SVG
    // loaded this way, and these files ship with the app rather than being
    // fetched, so there is no request and nothing to intercept.
    return <img src={icon.src} alt="" width={size} height={size} className="blueprint-icon-image" />;
  }

  if (icon.kind === "letters") {
    return (
      <svg width={size} height={size} viewBox="0 0 24 24" role="img" aria-hidden="true" focusable="false">
        <text
          x="12"
          y="12"
          fill={icon.tint}
          fontSize="11"
          fontWeight="700"
          textAnchor="middle"
          dominantBaseline="central"
          fontFamily="var(--font-sans)"
          letterSpacing="-0.5"
        >
          {icon.text}
        </text>
      </svg>
    );
  }

  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor" role="img" aria-hidden="true" focusable="false">
      <path d={icon.d} />
    </svg>
  );
}
