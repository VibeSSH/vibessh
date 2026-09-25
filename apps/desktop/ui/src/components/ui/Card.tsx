import { HTMLAttributes, ReactNode } from "react";
import "./Card.css";

interface CardProps extends Omit<HTMLAttributes<HTMLDivElement>, "title"> {
  title?: string;
  subtitle?: string;
  /** Buttons for the card as a whole - "Edit", "Add" - set at the right of
   *  the header, where they are found without scrolling past the content. */
  actions?: ReactNode;
}

export function Card({ title, subtitle, actions, children, className, ...rest }: CardProps) {
  const classes = ["card", className].filter(Boolean).join(" ");
  return (
    <div className={classes} {...rest}>
      {(title || subtitle || actions) && (
        <div className={actions ? "card-header card-header-with-actions" : "card-header"}>
          <div className="card-header-text">
            {title && <h3 className="card-title">{title}</h3>}
            {subtitle && <p className="card-subtitle">{subtitle}</p>}
          </div>
          {actions && <div className="card-actions">{actions}</div>}
        </div>
      )}
      <div className="card-body">{children}</div>
    </div>
  );
}
