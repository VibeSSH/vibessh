import { ButtonHTMLAttributes, forwardRef } from "react";
import "./Button.css";

type Variant = "primary" | "secondary" | "ghost" | "danger";

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant;
}

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(
  ({ variant = "primary", className, ...rest }, ref) => {
    const classes = ["btn", `btn-${variant}`, className].filter(Boolean).join(" ");
    return <button ref={ref} className={classes} {...rest} />;
  },
);

Button.displayName = "Button";
