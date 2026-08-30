import { ButtonHTMLAttributes, forwardRef } from "react";
import { useRipple } from "@/hooks/useRipple";
import "./Button.css";

type Variant = "primary" | "secondary" | "ghost" | "danger";

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant;
}

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(
  ({ variant = "primary", className, children, onPointerDown, ...rest }, ref) => {
    const { createRipple, rippleEls } = useRipple();
    const classes = ["btn", `btn-${variant}`, "ripple-host", className].filter(Boolean).join(" ");
    return (
      <button
        ref={ref}
        className={classes}
        onPointerDown={(e) => {
          createRipple(e);
          onPointerDown?.(e);
        }}
        {...rest}
      >
        {rippleEls}
        {children}
      </button>
    );
  },
);

Button.displayName = "Button";
