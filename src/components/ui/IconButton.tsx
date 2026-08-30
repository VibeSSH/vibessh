import { ButtonHTMLAttributes, forwardRef } from "react";
import { Icon } from "./Icon";
import { useRipple } from "@/hooks/useRipple";
import "./IconButton.css";

type Size = "sm" | "md";

interface IconButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  icon: string;
  iconSize?: number;
  size?: Size;
  danger?: boolean;
  /** Required, not optional - an icon-only button with no accessible name is exactly the gap docs/UI_AUDIT.md's accessibility pass exists to close. Used as both the native tooltip and aria-label. */
  title: string;
}

const DEFAULT_ICON_SIZE: Record<Size, number> = { sm: 14, md: 16 };

export const IconButton = forwardRef<HTMLButtonElement, IconButtonProps>(
  ({ icon, iconSize, size = "md", danger, title, className, onPointerDown, ...rest }, ref) => {
    const { createRipple, rippleEls } = useRipple();
    const classes = ["icon-btn", `icon-btn-${size}`, "ripple-host", danger ? "icon-btn-danger" : "", className]
      .filter(Boolean)
      .join(" ");
    return (
      <button
        ref={ref}
        className={classes}
        title={title}
        aria-label={title}
        onPointerDown={(e) => {
          createRipple(e);
          onPointerDown?.(e);
        }}
        {...rest}
      >
        {rippleEls}
        <Icon name={icon} size={iconSize ?? DEFAULT_ICON_SIZE[size]} />
      </button>
    );
  },
);

IconButton.displayName = "IconButton";
