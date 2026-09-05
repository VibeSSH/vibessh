import { FormEvent, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import {
  createServer,
  serverSummaryToManagedServer,
  testSshConnection,
  updateServer,
  type ServerFormInput,
} from "@/services/serverService";
import { useServersStore, type ManagedServer } from "@/stores/serversStore";
import { toastSuccess } from "@/stores/toastStore";
import type { AuthenticationType } from "@/types/server";
import "./forms.css";
import { errorMessage } from "@/services/tauri";

type TestStatus = "idle" | "testing" | "success" | "error";

interface SshServerFormProps {
  /** Present in edit mode - prefills the form and calls updateServer instead of createServer. */
  editingServer?: ManagedServer;
  onSaved: (server: ManagedServer) => void;
}

export function SshServerForm({ editingServer, onSaved }: SshServerFormProps) {
  const { t } = useTranslation();
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
  /**
   * The same "in flight" fact as `busy`, kept where it can be read in the
   * same tick it is written.
   *
   * `busy` disables the button, but only after React has re-rendered, and a
   * held-down Enter key repeats faster than that. Every repeat got through
   * the check and created another Node - somebody reported a dozen identical
   * ones from a single save. A ref closes the window because it is not state:
   * the guard below sees the value the line above it set.
   */
  const submitting = useRef(false);
  const [error, setError] = useState<string | null>(null);
  const [testStatus, setTestStatus] = useState<TestStatus>("idle");
  const [testMessage, setTestMessage] = useState<string | null>(null);

  function buildInput(): ServerFormInput {
    return {
      name: name.trim(),
      host: host.trim(),
      sshPort: Number(port) || 0,
      username: username.trim(),
      authenticationType: authMethod,
      privateKeyPath: authMethod === "privateKey" ? privateKeyPath.trim() : undefined,
      password: authMethod === "password" && password ? password : undefined,
      keyPassphrase: authMethod === "privateKey" && keyPassphrase ? keyPassphrase : undefined,
    };
  }

  async function handleTestConnection() {
    setTestStatus("testing");
    setTestMessage(null);
    try {
      await testSshConnection(buildInput());
      setTestStatus("success");
      setTestMessage(t("sshForm.testSuccess"));
    } catch (err) {
      setTestStatus("error");
      setTestMessage(errorMessage(err, t));
    }
  }

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    if (submitting.current) return;
    submitting.current = true;
    setBusy(true);
    setError(null);
    try {
      const input = buildInput();
      const saved = isEditing && editingServer
        ? await updateServer(editingServer.id, input)
        : await createServer(input);
      const managed = serverSummaryToManagedServer(saved);
      upsertServer(managed);
      toastSuccess(isEditing ? t("sshForm.savedChangesToast", { name: saved.name }) : t("sshForm.addedToast", { name: saved.name }));
      onSaved(managed);
    } catch (err) {
      setError(errorMessage(err, t));
    } finally {
      submitting.current = false;
      setBusy(false);
    }
  }

  return (
    <form className="server-form" onSubmit={handleSubmit}>
      <label className="form-field">
        <span className="form-label">{t("sshForm.name")}</span>
        <input
          className="form-input"
          placeholder={t("sshForm.namePlaceholder")}
          value={name}
          onChange={(e) => setName(e.target.value)}
          required
        />
      </label>

      <div className="form-row">
        <label className="form-field form-field-grow">
          <span className="form-label">{t("sshForm.host")}</span>
          <input
            className="form-input"
            placeholder={t("sshForm.hostPlaceholder")}
            value={host}
            onChange={(e) => setHost(e.target.value)}
            required
          />
        </label>
        <label className="form-field form-field-narrow">
          <span className="form-label">{t("sshForm.port")}</span>
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
        <span className="form-label">{t("sshForm.username")}</span>
        <input
          className="form-input"
          placeholder={t("sshForm.usernamePlaceholder")}
          value={username}
          onChange={(e) => setUsername(e.target.value)}
          required
        />
      </label>

      <div className="form-field">
        <span className="form-label">{t("sshForm.authentication")}</span>
        <div className="form-segmented">
          <button
            type="button"
            className={`form-segment ${authMethod === "password" ? "form-segment-active" : ""}`}
            onClick={() => setAuthMethod("password")}
          >
            {t("sshForm.password")}
          </button>
          <button
            type="button"
            className={`form-segment ${authMethod === "privateKey" ? "form-segment-active" : ""}`}
            onClick={() => setAuthMethod("privateKey")}
          >
            {t("sshForm.sshKey")}
          </button>
        </div>
        {authMethod === "password" ? (
          <input
            className="form-input"
            type="password"
            placeholder={isEditing ? t("sshForm.passwordPlaceholderEdit") : t("sshForm.passwordPlaceholder")}
            value={password}
            onChange={(e) => setPassword(e.target.value)}
          />
        ) : (
          <>
            <input
              className="form-input"
              placeholder={t("sshForm.keyPathPlaceholder")}
              value={privateKeyPath}
              onChange={(e) => setPrivateKeyPath(e.target.value)}
              required
            />
            <input
              className="form-input"
              type="password"
              placeholder={isEditing ? t("sshForm.passphrasePlaceholderEdit") : t("sshForm.passphrasePlaceholder")}
              value={keyPassphrase}
              onChange={(e) => setKeyPassphrase(e.target.value)}
            />
          </>
        )}
        <p className="form-note">{authMethod === "privateKey" ? t("sshForm.noteKey") : t("sshForm.notePassword")}</p>
      </div>

      {testMessage && (
        <p className={`form-note ${testStatus === "success" ? "form-note-success" : "form-note-danger"}`}>{testMessage}</p>
      )}

      {error && <p className="form-note form-note-danger">{error}</p>}

      <div className="form-actions form-actions-split">
        <Button type="button" variant="secondary" onClick={handleTestConnection} disabled={testStatus === "testing" || busy}>
          {testStatus === "testing" ? t("sshForm.testing") : t("sshForm.testConnection")}
        </Button>
        <Button type="submit" disabled={busy}>
          {isEditing ? t("sshForm.saveChanges") : t("sshForm.saveServer")}
        </Button>
      </div>
      <p className="form-note">{isEditing ? t("sshForm.noteEdit") : t("sshForm.noteAdd")}</p>
    </form>
  );
}
