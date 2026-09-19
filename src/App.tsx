import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { api, asAppError, onLog, onStatus } from "./api";
import type { AppConfig, AppPrefs, ConfigPayload, DeviceEntry, EngineStatus, LanguageSetting, LogLine, Theme } from "./bindings";
import { applyLanguage, resolveLanguage } from "./i18n";
import { AdvancedTab } from "./tabs/AdvancedTab";
import { AppsTab } from "./tabs/AppsTab";
import { AudioTab } from "./tabs/AudioTab";
import { ColorsTab } from "./tabs/ColorsTab";
import { DevicesTab } from "./tabs/DevicesTab";
import { DynamicsTab } from "./tabs/DynamicsTab";
import { OutputTab } from "./tabs/OutputTab";
import { SpatialTab } from "./tabs/SpatialTab";
import { StatusTab } from "./tabs/StatusTab";

const TABS = ["status", "audio", "apps", "colors", "dynamics", "output", "spatial", "devices", "advanced"] as const;

type TabId = (typeof TABS)[number];

export type Notice = { tone: "ok" | "warn" | "error"; title: string; lines?: string[] };

const LOG_LIMIT = 500;

export function App() {
  const { t } = useTranslation();

  const [payload, setPayload] = useState<ConfigPayload | null>(null);
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [savedJson, setSavedJson] = useState("");
  const [status, setStatus] = useState<EngineStatus | null>(null);
  const [logs, setLogs] = useState<LogLine[]>([]);
  const [prefs, setPrefs] = useState<AppPrefs | null>(null);
  const [tab, setTab] = useState<TabId>("status");
  const [notice, setNotice] = useState<Notice | null>(null);
  const [busy, setBusy] = useState(false);

  const dirty = config !== null && JSON.stringify(config) !== savedJson;
  const dirtyRef = useRef(dirty);
  dirtyRef.current = dirty;

  const reload = useCallback(async () => {
    try {
      const next = await api.loadConfig();
      setPayload(next);
      setConfig(next.config);
      setSavedJson(JSON.stringify(next.config));
      if (next.recovered) {
        setNotice({
          tone: "error",
          title: t("load.brokenTitle", { error: next.recovered.error }),
          lines: [
            next.recovered.backup ? t("load.brokenBackup", { path: next.recovered.backup }) : t("load.brokenNoBackup"),
            t("load.brokenDefaults"),
          ],
        });
      } else if (next.missing) {
        setNotice({ tone: "warn", title: t("load.missingTitle", { path: next.path }), lines: [t("load.missingHint")] });
      } else if (next.warnings.length > 0) {
        setNotice({ tone: "warn", title: t("load.warningsTitle"), lines: next.warnings });
      }
    } catch (e) {
      setNotice({ tone: "error", title: asAppError(e).message });
    }
  }, [t]);

  useEffect(() => {
    void reload();
    void api.engineStatus().then(setStatus);
    void api.recentLogs().then(setLogs);

    void api.getPrefs().then((loaded) => {
      setPrefs(loaded);
      if (loaded.language === "system") void api.setLanguage("system", applyLanguage(loaded.language)).catch(() => undefined);
    });

    const unlisten = Promise.all([onStatus(setStatus), onLog((line) => setLogs((current) => [...current, line].slice(-LOG_LIMIT)))]);
    return () => {
      void unlisten.then((fns) => fns.forEach((fn) => fn()));
    };
  }, [reload]);

  useEffect(() => {
    const theme = prefs?.theme ?? "system";
    if (theme === "system") document.documentElement.removeAttribute("data-theme");
    else document.documentElement.setAttribute("data-theme", theme);
  }, [prefs?.theme]);

  const save = useCallback(
    async (thenReload: boolean) => {
      if (!config) return;
      setBusy(true);
      try {
        const warnings = await api.saveConfig(config);
        setSavedJson(JSON.stringify(config));
        if (thenReload) await api.engineReload();
        setNotice(warnings.length > 0 ? { tone: "warn", title: thenReload ? t("save.savedReloadedWarn") : t("save.savedWarn"), lines: warnings } : { tone: "ok", title: thenReload ? t("save.savedReloaded") : t("save.saved") },);
      } catch (e) {
        setNotice({ tone: "error", title: asAppError(e).message });
      } finally {
        setBusy(false);
      }
    },
    [config, t],
  );

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "s") {
        e.preventDefault();
        if (dirtyRef.current) void save(false);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [save]);

  const setTheme = useCallback(async (theme: Theme) => {
    setPrefs((current) => (current ? { ...current, theme } : current));
    try {
      await api.setTheme(theme);
    } catch (e) {
      setNotice({ tone: "error", title: asAppError(e).message });
    }
  }, []);

  const setLanguage = useCallback(async (language: LanguageSetting) => {
    setPrefs((current) => (current ? { ...current, language } : current));
    const effective = applyLanguage(language);
    try {
      await api.setLanguage(language, effective);
    } catch (e) {
      setNotice({ tone: "error", title: asAppError(e).message });
    }
  }, []);

  const title = useMemo(() => t(`nav.${tab}`), [t, tab]);

  return (
    <div className="shell">
      <aside className="sidebar">
        <div className="brand">
          <b>{t("app.title")}</b>
        </div>
        <nav className="nav">
          {TABS.map((id) => (
            <button key={id} type="button" aria-current={tab === id ? "page" : undefined} onClick={() => setTab(id)}>
              {t(`nav.${id}`)}
            </button>
          ))}
        </nav>
        <div className="engine-pill">
          <span className={`dot ${status?.state ?? "stopped"}`} />
          <span>{t(`engine.${status?.state ?? "stopped"}`)}</span>
        </div>
      </aside>

      <main className="main">
        <div className="topbar">
          <h1>{title}</h1>
          <span className="hint">{payload?.path ?? "…"}</span>
          <span className="spacer" />
        </div>

        <div className="content">
          {notice && <NoticeBox notice={notice} onDismiss={() => setNotice(null)} />}

          {!config ? (
            <p className="prose">{t("app.loading")}</p>
          ) : (
            <TabBody
              tab={tab}
              config={config}
              onChange={setConfig}
              status={status}
              logs={logs}
              prefs={prefs}
              onTheme={setTheme}
              onLanguage={setLanguage}
              onReloadConfig={reload}
              path={payload?.path ?? ""}
              onNotice={setNotice}
            />
          )}
        </div>

        <div className="savebar">
          <span className={dirty ? "dirty" : "prose"}>{dirty ? t("save.dirty") : t("save.clean")}</span>
          <span className="spacer" />
          <button type="button" className="btn" disabled={!dirty || busy} onClick={() => config && reload()}>
            {t("save.discard")}
          </button>
          <button type="button" className="btn" disabled={!dirty || busy} onClick={() => void save(false)}>
            {t("save.save")}
          </button>
          <button type="button" className="btn primary" disabled={busy} onClick={() => void save(true)}>
            {t("save.saveReload")}
          </button>
        </div>
      </main>
    </div>
  );
}

function TabBody(props: {
  tab: TabId;
  config: AppConfig;
  onChange: (next: AppConfig) => void;
  status: EngineStatus | null;
  logs: LogLine[];
  prefs: AppPrefs | null;
  onTheme: (theme: Theme) => void;
  onLanguage: (language: LanguageSetting) => void;
  onReloadConfig: () => Promise<void>;
  path: string;
  onNotice: (notice: Notice | null) => void;
}) {
  const { tab, config, onChange } = props;
  const set =
    <K extends keyof AppConfig>(key: K) =>
    (value: AppConfig[K]) =>
      onChange({ ...config, [key]: value });

  switch (tab) {
    case "status":
      return <StatusTab status={props.status} logs={props.logs} onNotice={props.onNotice} />;
    case "audio":
      return <AudioTab audio={config.audio} onChange={set("audio")} onNotice={props.onNotice} />;
    case "apps":
      return <AppsTab audio={config.audio} onChange={set("audio")} onNotice={props.onNotice} />;
    case "colors":
      return <ColorsTab bands={config.bands} colorMap={config.color_map} onBands={set("bands")} onColorMap={set("color_map")} />;
    case "dynamics":
      return <DynamicsTab dynamics={config.dynamics} onChange={set("dynamics")} />;
    case "output":
      return <OutputTab output={config.output} onChange={set("output")} />;
    case "spatial":
      return (
        <SpatialTab
          spatial={config.spatial}
          onChange={set("spatial")}
          devices={config.devices as unknown as DeviceEntry[]}
          onDevices={(next) => onChange({ ...config, devices: next as unknown as AppConfig["devices"] })}
          status={props.status}
        />
      );
    case "devices":
      return (
        <DevicesTab
          devices={config.devices as unknown as DeviceEntry[]}
          onChange={(next) => onChange({ ...config, devices: next as unknown as AppConfig["devices"] })}
          onNotice={props.onNotice}
        />
      );
    case "advanced":
      return (
        <AdvancedTab
          config={config}
          onChange={onChange}
          path={props.path}
          prefs={props.prefs}
          onTheme={props.onTheme}
          onLanguage={props.onLanguage}
          onReloadConfig={props.onReloadConfig}
          onNotice={props.onNotice}
        />
      );
  }
}

function NoticeBox({ notice, onDismiss }: { notice: Notice; onDismiss: () => void }) {
  const { t } = useTranslation();
  return (
    <div className={`notice ${notice.tone}`}>
      <div className="inline">
        <h3>{notice.title}</h3>
        <span className="spacer" style={{ flex: 1 }} />
        <button type="button" className="btn small" onClick={onDismiss}>
          {t("app.dismiss")}
        </button>
      </div>
      {notice.lines && notice.lines.length > 0 && (
        <ul className="prose">
          {notice.lines.map((line, i) => (
            <li key={i}>{line}</li>
          ))}
        </ul>
      )}
    </div>
  );
}

export { resolveLanguage };
