import { useState } from "react";
import { Button } from "@/components/ui/Button";
import "./forms.css";

type AuthMethod = "password" | "privateKey";

export function SshServerForm() {
  const [authMethod, setAuthMethod] = useState<AuthMethod>("password");

  return (
    <form className="server-form" onSubmit={(e) => e.preventDefault()}>
      <label className="form-field">
        <span className="form-label">Name</span>
        <input className="form-input" placeholder="Production server" />
      </label>

      <div className="form-row">
        <label className="form-field form-field-grow">
          <span className="form-label">Host</span>
          <input className="form-input" placeholder="203.0.113.10" />
        </label>
        <label className="form-field form-field-narrow">
          <span className="form-label">Port</span>
          <input className="form-input" placeholder="22" />
        </label>
      </div>

      <label className="form-field">
        <span className="form-label">Username</span>
        <input className="form-input" placeholder="root" />
      </label>

      <div className="form-field">
        <span className="form-label">Authentication</span>
        <div className="form-segmented">
          <button
            type="button"
            className={`form-segment ${authMethod === "password" ? "form-segment-active" : ""}`}
            onClick={() => setAuthMethod("password")}
          >
            Password
          </button>
          <button
            type="button"
            className={`form-segment ${authMethod === "privateKey" ? "form-segment-active" : ""}`}
            onClick={() => setAuthMethod("privateKey")}
          >
            SSH Key
          </button>
        </div>
        {authMethod === "password" ? (
          <input className="form-input" type="password" placeholder="••••••••" />
        ) : (
          <textarea className="form-input form-textarea" placeholder="-----BEGIN OPENSSH PRIVATE KEY-----" />
        )}
      </div>

      <div className="form-actions">
        <Button type="submit" disabled title="Server storage and SshTransport land in Etap 2/3">
          Test connection &amp; save
        </Button>
      </div>
      <p className="form-note">
        Not wired up yet — server storage (Etap 2) and the SSH transport
        (Etap 3) aren't built. This form is real UI, not a mock, waiting on
        a backend to submit to.
      </p>
    </form>
  );
}
