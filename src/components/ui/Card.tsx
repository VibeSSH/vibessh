import { HTMLAttributes } from "react";
import "./Card.css";

interface CardProps extends HTMLAttributes<HTMLDivElement> {
  title?: string;
  subtitle?: string;
}

export function Card({ title, subtitle, children, className, ...rest }: CardProps) {
  const classes = ["card", className].filter(Boolean).join(" ");
  return (
    <div className={classes} {...rest}>
      {(title || subtitle) && (
        <div className="card-header">
          {title && <h3 className="card-title">{title}</h3>}
          {subtitle && <p className="card-subtitle">{subtitle}</p>}
        </div>
      )}
      <div className="card-body">{children}</div>
    </div>
  );
}
