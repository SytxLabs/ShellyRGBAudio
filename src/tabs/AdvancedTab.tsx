import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import type { Notice } from "../App";
import { api, asAppError } from "../api";
import type { AppConfig, AppPrefs, LanguageSetting, Theme } from "../bindings";
import { CheckboxField, Field, Section, SelectField } from "../components/fields";

export function AdvancedTab({config, onChange, path, prefs, onTheme, onLanguage, onReloadConfig, onNotice,}: {
  config: AppConfig;
  onChange: (next: AppConfig) => void;
  path: string;
  prefs: AppPrefs | null;
  onTheme: (theme: Theme) => void;
  onLanguage: (language: LanguageSetting) => void;
  onReloadConfig: () => Promise<void>;
  onNotice: (n: Notice | null) => void;
}) {
  const { t } = useTranslation();
  const [target, setTarget] = useState(path);
  const [moveFile, setMoveFile] = useState(true);
  const [busy, setBusy] = useState(false);

  useEffect(() => setTarget(path), [path]);

  const themes: readonly (readonly [Theme, string])[] = [
    ["system", t("advanced.themes.system")],
    ["light", t("advanced.themes.light")],
    ["dark", t("advanced.themes.dark")],
  ];
  const languages: readonly (readonly [LanguageSetting, string])[] = [
    ["system", t("advanced.languages.system")],
    ["en", t("advanced.languages.en")],
    ["de", t("advanced.languages.de")],
  ];

  const pick = async () => {
    const picked = await open({multiple: false, directory: false, defaultPath: path, filters: [{ name: "JSON", extensions: ["json"] }]});
    if (typeof picked === "string") setTarget(picked);
  };

  const apply = async () => {
    setBusy(true);
    try {
      const applied = await api.setConfigPath(target, moveFile);
      await onReloadConfig();
      onNotice({ tone: "ok", title: t("advanced.pathApplied", { path: applied }) });
    } catch (e) {
      onNotice({ tone: "error", title: asAppError(e).message });
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <Section title={t("advanced.appearanceTitle")}>
        <SelectField label={t("advanced.theme")} note={t("advanced.themeNote")} value={prefs?.theme ?? "system"} onChange={onTheme} options={themes} />
        <SelectField
          label={t("advanced.language")}
          note={t("advanced.languageNote")}
          value={prefs?.language ?? "system"}
          onChange={onLanguage}
          options={languages}
        />
      </Section>

      <Section title={t("advanced.pathTitle")} note={t("advanced.pathNote")}>
        <Field label={t("advanced.file")}>
          <div className="inline">
            <input type="text" value={target} spellCheck={false} onChange={(e) => setTarget(e.target.value)} />
            <button type="button" className="btn small" onClick={() => void pick()}>
              {t("advanced.browse")}
            </button>
          </div>
        </Field>
        <CheckboxField label={t("advanced.move")} text={t("advanced.moveText")} value={moveFile} onChange={setMoveFile} />
        <Field>
          <button type="button" className="btn" disabled={busy || target === path || target.trim() === ""} onClick={() => void apply()}>
            {t("advanced.apply")}
          </button>
        </Field>
      </Section>

      <Section title={t("advanced.jsonTitle")} note={t("advanced.jsonNote")}>
        <JsonEditor config={config} onChange={onChange} />
      </Section>

      <Section title={t("advanced.resetTitle")}>
        <Field label={t("advanced.resetLabel")} note={t("advanced.resetNote")}>
          <button
            type="button"
            className="btn danger"
            onClick={() => {
              onChange(defaults(config));
              onNotice({ tone: "warn", title: t("advanced.resetDone"), lines: [t("advanced.resetDoneNote")] });
            }}
          >
            {t("advanced.reset")}
          </button>
        </Field>
      </Section>
    </>
  );
}

function defaults(current: AppConfig): AppConfig {
  return {
    audio: {
      device: { type: "default" },
      apps: { enabled: false, mode: "include", include_process_tree: true, rescan_ms: 3000, sample_rate: 48000, channels: 2, groups: [] },
      fft_size: 1024,
      hop_size: 1024,
      window: "hann",
      downmix: "average",
      buffer_duration_hns: 200000,
      silence_timeout_ms: 2000,
      silence_fade_ms: 800,
      silence_brightness: 0,
    },
    bands: [{ name: "bass", from_hz: 20, to_hz: 200, weight: 1, color: null }, { name: "mid", from_hz: 200, to_hz: 2000, weight: 1, color: null }, { name: "treble", from_hz: 2000, to_hz: 8000, weight: 1, color: null },],
    color_map: {
      stops: [{ hz: 63, color: "#FF0000" }, { hz: 632, color: "#00FF00" }, { hz: 4000, color: "#0000FF" }],
      frequency_scale: "log",
      interpolation: "srgb",
      saturation: 1,
      value_floor: 0.15,
      value_span: 0.85,
      white_channel: "off",
      white_fixed: 0,
      fallback_color: "#FF0000",
    },
    dynamics: {
      normalize: "shared",
      level_source: "peak",
      log_offset: 1,
      peak_floor: 0.000001,
      peak_decay: 0.995,
      level_alpha: 0.12,
      band_attack: 0.45,
      band_release: 0.12,
      flux_alpha: 0.25,
      beat_threshold: 0.18,
      beat_cooldown_ms: 0,
      strobe_ms: 40,
      strobe_level: 1,
      strobe_color: null,
    },
    output: {
      change_interval_ms: 120,
      transition_min_ms: 60,
      transition_max_ms: 600,
      transition_beat_weight: 0.75,
      transition_level_weight: 0.25,
      transition_curve: 1,
      deadband_rgb: 3,
      deadband_overall: 0.02,
      brightness_floor: 1,
      gamma_min: 0.1,
      gamma_max: 5,
    },
    spatial: {
      enabled: false,
      layout: "auto",
      room: { min: [-3, 0, -3], max: [3, 3, 3] },
      focus: 2,
      omni_floor: 0.15,
      strip_samples: 8,
      height_sharpness: 0.35,
      distance_falloff: 0,
    },
    devices: current.devices,
  };
}

function JsonEditor({ config, onChange }: { config: AppConfig; onChange: (next: AppConfig) => void }) {
  const { t } = useTranslation();
  const serialized = JSON.stringify(config, null, 2);
  const [text, setText] = useState(serialized);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    try {
      if (JSON.stringify(JSON.parse(text)) !== JSON.stringify(config)) setText(serialized);
    } catch {
      /* mid-edit and unparseable; leave the buffer alone */
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [serialized]);

  return (
    <Field wide note={error ?? t("advanced.jsonValid")}>
      <textarea
        spellCheck={false}
        style={{ minHeight: 420 }}
        value={text}
        onChange={(e) => {
          setText(e.target.value);
          try {
            const parsed: unknown = JSON.parse(e.target.value);
            if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) throw new Error(t("devices.jsonObject"));
            setError(null);
            onChange(parsed as AppConfig);
          } catch (e) {
            setError(e instanceof Error ? e.message : String(e));
          }
        }}
      />
    </Field>
  );
}
