import { FormEvent, useState } from "react";
import { Button } from "@/components/ui/Button";
import { createServer, serverSummaryToManagedServer, updateServer } from "@/services/serverService";
import { useServersStore, type ManagedServer } from "@/stores/serversStore";
import type { AuthenticationType } from "@/types/server";
import "./forms.css";

interface SshServerFormProps {
  /** Present in edit mode - prefills the form and calls updateServer instead of createServer. */
  editingServer?: ManagedServer;
  onSaved: () => void;
}

export function SshServerForm({ editingServer, onSaved }: SshServerFormProps) {
  const isEditing = Boolean(editingServer);
  const upsertServer = useServersStore((s) => s.upsertServer);

  const [name, setName] = useState(editingServer?.name ?? "");
  const [host, setHost] = useState(editingServer?.host ?? "");
  const [port, setPort] = useState(String(editingServer?.sshPort ?? 22));
  const [username, setUsername] = useState(editingServer?.username ?? "");
  const [authMethod, setAuthMethod] = useState<AuthenticationType>(
    editingServer?.authenticationType ?? "password",
  );
  const [password, setPassword] = useState("");
  const [privateKeyPath, setPrivateKeyPath] = useState(editingServer?.privateKeyPath ?? "");
  const [keyPassphrase, setKeyPassphrase] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const input = {
        name: name.trim(),
        host: host.trim(),
        sshPort: Number(port) || 0,
        username: username.trim(),
        authenticationType: authMethod,
        privateKeyPath: authMethod === "privateKey" ? privateKeyPath.trim() : undefined,
        password: authMethod === "password" && password ? password : undefined,
        keyPassphrase: authMethod === "privateKey" && keyPassphrase ? keyPassphrase : undefined,
      };
      const saved = isEditing && editingServer
        ? await updateServer(editingServer.id, input)
        : await createServer(input);
      upsertServer(serverSummaryToManagedServer(saved));
      onSaved();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Couldn't save the server.");
    } finally {
      setBusy(false);
    }
  }

  return (
    <form className="server-form" onSubmit={handleSubmit}>
      <label className="form-field">
        <span className="form-label">Name</span>
        <input
          className="form-input"
          placeholder="Production server"
          value={name}
          onChange={(e) => setName(e.target.value)}
          required
        />
      </label>

      <div className="form-row">
        <label className="form-field form-field-grow">
          <span className="form-label">Host</span>
          <input
            className="form-input"
            placeholder="203.0.113.10"
            value={host}
            onChange={(e) => setHost(e.target.value)}
            required
          />
        </label>
        <label className="form-field form-field-narrow">
          <span className="form-label">Port</span>
          <input
            className="form-input"
            value={port}
            onChange={(e) => setPort(e.target.value)}
            inputMode="numeric"
            required
          />
        </label>
      </div>

      <label className="form-field">
        <span className="form-label">Username</span>
        <input
          className="form-input"
          placeholder="root"
          value={username}
          onChange={(e) => setUsername(e.target.value)}
          required
        />
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
          <input
            className="form-input"
            type="password"
            placeholder={isEditing ? "Leave blank to keep the current password" : "••••••••"}
            value={password}
            onChange={(e) => setPassword(e.target.value)}
          />
        ) : (
          <>
            <input
              className="form-input"
              placeholder="C:\Users\you\.ssh\id_ed25519"
              value={privateKeyPath}
              onChange={(e) => setPrivateKeyPath(e.target.value)}
              required
            />
            <input
              className="form-input"
              type="password"
              placeholder={isEditing ? "Leave blank to keep the current passphrase" : "Passphrase (optional)"}
              value={keyPassphrase}
              onChange={(e) => setKeyPassphrase(e.target.value)}
              style={{ marginTop: 8 }}
            />
          </>
        )}
        <p className="form-note">
          {authMethod === "privateKey"
            ? "VibeSSH reads the key from this path on disk when it connects - it never copies the file's contents."
            : "The password is stored in your OS credential store, never in a plain file."}
        </p>
      </div>

      {error && (
        <p className="form-note" style={{ color: "var(--danger)" }}>
          {error}
        </p>
      )}

      <div className="form-actions">
        <Button type="submit" disabled={busy}>
          {isEditing ? "Save changes" : "Save server"}
        </Button>
      </div>
      <p className="form-note">
        Etap 3's real SSH transport isn't built yet, so saving stores the
        server for later - it doesn't test the connection.
      </p>
    </form>
  );
}
