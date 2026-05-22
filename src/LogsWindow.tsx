import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { t } from "./i18n.js";

type Settings = { autostart: boolean; language: string | null };

export default function LogsWindow() {
  const [lines, setLines] = useState<string[]>([]);
  const [follow, setFollow] = useState(true);
  const [lang, setLang] = useState("en");
  const endRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    invoke<Settings>("cmd_get_settings").then((s) => {
      const detected = navigator.language?.startsWith("es") ? "es" : "en";
      setLang(s.language ?? detected);
    });
    invoke<string[]>("cmd_get_log_snapshot").then(setLines);
    const unlisten = listen<string>("hub-log-line", (e) => {
      setLines((prev) => [...prev.slice(-4999), e.payload]);
    });
    return () => { unlisten.then((u) => u()); };
  }, []);

  useEffect(() => {
    if (follow) endRef.current?.scrollIntoView({ behavior: "instant" as ScrollBehavior });
  }, [lines, follow]);

  return (
    <div style={{
      display: "flex", flexDirection: "column", height: "100vh",
      fontFamily: "ui-monospace, SFMono-Regular, Menlo, Consolas, monospace", fontSize: 12,
    }}>
      <div style={{ padding: 8, borderBottom: "1px solid #ddd", display: "flex", gap: 12 }}>
        <strong>{t(lang, "logs.title")}</strong>
        <label><input type="checkbox" checked={follow} onChange={(e) => setFollow(e.target.checked)} /> {t(lang, "logs.follow")}</label>
        <button onClick={() => setLines([])}>{t(lang, "logs.clear")}</button>
      </div>
      <div style={{ flex: 1, overflow: "auto", padding: 8, whiteSpace: "pre-wrap" }}>
        {lines.length === 0 ? <div style={{ opacity: 0.5 }}>{t(lang, "logs.empty")}</div> : null}
        {lines.map((l, i) => <div key={i}>{l}</div>)}
        <div ref={endRef} />
      </div>
    </div>
  );
}
