import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { api, asAppError } from "../api";
import type { EngineStatus, LogLine } from "../bindings";
import type { Notice } from "../App";
import { Section } from "../components/fields";
import { formatSampleRate } from "../util";

export function StatusTab({ status, logs, onNotice }: { status: EngineStatus | null; logs: LogLine[]; onNotice: (n: Notice | null) => void }) {
  const { t } = useTranslation();
  const [busy, setBusy] = useState(false);
  const running = status?.state === "running" || status?.state === "starting";

  const act = async (what: "start" | "stop" | "reload") => {
    setBusy(true);
    try {
      if (what === "start") await api.engineStart();
      if (what === "stop") await api.engineStop();
      if (what === "reload") await api.engineReload();
    } catch (e) {
      onNotice({ tone: "error", title: asAppError(e).message });
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <div className="stats">
        <Stat k={t("status.state")} v={t(`engine.${status?.state ?? "stopped"}`)} />
        <Stat k={t("status.source")} v={status?.source ?? "–"} />
        <Stat k={t("status.sampleRate")} v={status?.sample_rate ? formatSampleRate(status.sample_rate) : "–"} />
        <Stat k={t("status.channels")} v={status?.layout ?? (status?.channels ? String(status.channels) : "–")} />
        <Stat k={t("status.devices")} v={status?.devices.length ? String(status.devices.length) : "0"} />
      </div>

      <div className="inline" style={{ marginBottom: 20 }}>
        <button type="button" className="btn primary" disabled={busy} onClick={() => void act(running ? "stop" : "start")}>
          {running ? t("status.pause") : t("status.resume")}
        </button>
        <button type="button" className="btn" disabled={busy} onClick={() => void act("reload")}>
          {t("status.reload")}
        </button>
        <span className="prose">{t("status.reloadHint")}</span>
      </div>

      {status?.error && (
        <div className="notice error">
          <h3>{t("status.failedTitle")}</h3>
          <p className="prose">{status.error}</p>
        </div>
      )}

      {status && status.warnings.length > 0 && (
        <div className="notice warn">
          <h3>{t("status.warningsTitle")}</h3>
          <ul className="prose">
            {status.warnings.map((w, i) => (
              <li key={i}>{w}</li>
            ))}
          </ul>
        </div>
      )}

      <Section title={t("status.devicesTitle")}>
        {status && status.devices.length > 0 ? (
          <table className="grid">
            <tbody>
              {status.devices.map((name, i) => (
                <tr key={`${name}-${i}`}>
                  <td>{name}</td>
                </tr>
              ))}
            </tbody>
          </table>
        ) : (
          <p className="prose">{t("status.noDevices")}</p>
        )}
      </Section>

      <Section title={t("status.logTitle")} note={t("status.logNote")}>
        <LogView logs={logs} empty={t("status.logEmpty")} />
      </Section>
    </>
  );
}

function Stat({ k, v }: { k: string; v: string }) {
  return (
    <div className="stat">
      <span className="k">{k}</span>
      <span className="v">{v}</span>
    </div>
  );
}

function LogView({ logs, empty }: { logs: LogLine[]; empty: string }) {
  const box = useRef<HTMLDivElement>(null);
  const pinned = useRef(true);

  // Follow new lines only while the reader is already at the bottom, so scrolling back to read something does not get yanked away.
  useEffect(() => {
    const el = box.current;
    if (el && pinned.current) el.scrollTop = el.scrollHeight;
  }, [logs]);

  return (
    <div
      className="log"
      ref={box}
      onScroll={(e) => {
        const el = e.currentTarget;
        pinned.current = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
      }}
    >
      {logs.length === 0 ? (
        <div className="empty">{empty}</div>
      ) : (
        logs.map((line, i) => (
          <div key={i} className={line.level}>
            {line.message}
          </div>
        ))
      )}
    </div>
  );
}
