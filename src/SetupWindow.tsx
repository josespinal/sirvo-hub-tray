import { useEffect, useState, type CSSProperties, type FormEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { t } from "./i18n.js";

type Settings = { autostart: boolean; language: string | null };
type Storage = "keychain" | "file";
type OdooConn = { url: string; db: string; user: string };
type ConnView = { conn: OdooConn | null; has_password: boolean; storage: Storage | null };
type SaveOutcome = { storage: Storage; hub_service_group: boolean };
type ConnError = { code: string; detail: string | null };

/** "Connection to Odoo": the hub's URL, database, user and password. */
export default function SetupWindow() {
  const [lang, setLang] = useState("en");
  const [url, setUrl] = useState("");
  const [db, setDb] = useState("");
  const [user, setUser] = useState("");
  const [password, setPassword] = useState("");
  const [saved, setSaved] = useState<OdooConn | null>(null);
  const [hasPassword, setHasPassword] = useState(false);
  const [storage, setStorage] = useState<Storage | null>(null);
  const [configError, setConfigError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<ConnError | null>(null);
  const [outcome, setOutcome] = useState<SaveOutcome | null>(null);

  useEffect(() => {
    invoke<Settings>("cmd_get_settings").then((s) => {
      const detected = navigator.language?.startsWith("es") ? "es" : "en";
      setLang(s.language ?? detected);
    });
    invoke<ConnView>("cmd_get_odoo_conn").then((view) => {
      if (view.conn) {
        setUrl(view.conn.url);
        setDb(view.conn.db);
        setUser(view.conn.user);
      }
      setSaved(view.conn);
      setHasPassword(view.has_password);
      setStorage(view.storage);
    });
    invoke<string | null>("cmd_get_config_error").then(setConfigError);
  }, []);

  // The stored password belongs to one server/database/user; it can only be
  // kept while those are unchanged.
  const sameAccount =
    saved !== null && saved.url === url.trim().replace(/\/+$/, "") && saved.db === db.trim() && saved.user === user.trim();
  const canKeepPassword = hasPassword && sameAccount;

  async function submit(e: FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    setOutcome(null);
    try {
      const result = await invoke<SaveOutcome>("cmd_save_odoo_conn", {
        url,
        db,
        user,
        password: password === "" ? null : password,
      });
      setOutcome(result);
      setSaved({ url: url.trim().replace(/\/+$/, ""), db: db.trim(), user: user.trim() });
      setHasPassword(true);
      setStorage(result.storage);
      setPassword("");
      setConfigError(null);
    } catch (err) {
      setError(typeof err === "object" && err !== null && "code" in err ? (err as ConnError) : { code: "unknown", detail: String(err) });
    } finally {
      setBusy(false);
    }
  }

  return (
    <form onSubmit={submit} style={styles.page}>
      <h2 style={{ margin: 0 }}>{t(lang, "setup.title")}</h2>
      <p style={styles.muted}>{t(lang, "setup.intro")}</p>

      {configError ? (
        <div style={{ ...styles.banner, ...styles.bad }}>
          <strong>{t(lang, "setup.hubRefused")}</strong>
          <div>{configError}</div>
        </div>
      ) : null}

      <label style={styles.field}>
        {t(lang, "setup.url")}
        <input style={styles.input} value={url} onChange={(e) => setUrl(e.target.value)} placeholder="https://erp.example.com" autoFocus required />
      </label>
      <label style={styles.field}>
        {t(lang, "setup.db")}
        <input style={styles.input} value={db} onChange={(e) => setDb(e.target.value)} required />
      </label>
      <label style={styles.field}>
        {t(lang, "setup.user")}
        <input style={styles.input} value={user} onChange={(e) => setUser(e.target.value)} placeholder="hub-barilo" autoComplete="off" required />
      </label>
      <label style={styles.field}>
        {t(lang, "setup.password")}
        <input
          style={styles.input}
          type="password"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          placeholder={canKeepPassword ? t(lang, "setup.passwordKeep") : ""}
          autoComplete="new-password"
          required={!canKeepPassword}
        />
      </label>
      <p style={styles.muted}>{t(lang, "setup.userHint")}</p>

      {error ? (
        <div style={{ ...styles.banner, ...styles.bad }}>
          <strong>{t(lang, `setup.errors.${error.code}`)}</strong>
          {error.detail ? <div style={styles.detail}>{error.detail}</div> : null}
        </div>
      ) : null}

      {outcome ? (
        <div style={{ ...styles.banner, ...(outcome.hub_service_group ? styles.good : styles.warn) }}>
          <strong>{t(lang, "setup.saved")}</strong>
          {outcome.hub_service_group ? null : <div>{t(lang, "setup.noHubGroup")}</div>}
        </div>
      ) : null}

      {storage === "file" ? <div style={{ ...styles.banner, ...styles.warn }}>{t(lang, "setup.storedInFile")}</div> : null}
      {storage === "keychain" && !outcome ? <p style={styles.muted}>{t(lang, "setup.storedInKeychain")}</p> : null}

      <div style={styles.actions}>
        <button type="button" onClick={() => getCurrentWindow().close()} disabled={busy}>
          {t(lang, "setup.close")}
        </button>
        <button type="submit" disabled={busy}>
          {busy ? t(lang, "setup.testing") : t(lang, "setup.testAndSave")}
        </button>
      </div>
    </form>
  );
}

const styles: Record<string, CSSProperties> = {
  page: {
    display: "flex", flexDirection: "column", gap: 10, padding: 20,
    fontFamily: "system-ui, -apple-system, Segoe UI, sans-serif", fontSize: 14,
  },
  muted: { margin: 0, opacity: 0.7, fontSize: 13 },
  field: { display: "flex", flexDirection: "column", gap: 4 },
  input: { padding: "6px 8px", fontSize: 14 },
  banner: { padding: "8px 10px", borderRadius: 6, display: "flex", flexDirection: "column", gap: 4, fontSize: 13 },
  bad: { background: "#fde8e8", color: "#8a1c1c" },
  warn: { background: "#fff4d6", color: "#7a5500" },
  good: { background: "#e3f6e8", color: "#1d6b34" },
  detail: { fontFamily: "ui-monospace, Menlo, Consolas, monospace", fontSize: 12, wordBreak: "break-word" },
  actions: { display: "flex", justifyContent: "flex-end", gap: 8, marginTop: 6 },
};
