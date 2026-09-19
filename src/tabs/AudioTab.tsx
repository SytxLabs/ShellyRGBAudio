import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import type { Notice } from "../App";
import { api, asAppError } from "../api";
import type { AudioDeviceInfo, AudioDeviceSelector, AudioSection, Downmix, WindowKind } from "../bindings";
import { CheckboxField, Field, NumberField, Section, SelectField, SliderField } from "../components/fields";
import { patcher } from "../util";

const FFT_SIZES = [256, 512, 1024, 2048, 4096, 8192];

export function AudioTab({
  audio,
  onChange,
  onNotice,
}: {
  audio: AudioSection;
  onChange: (next: AudioSection) => void;
  onNotice: (n: Notice | null) => void;
}) {
  const { t } = useTranslation();
  const set = patcher(audio, onChange);

  const windows: readonly (readonly [WindowKind, string])[] = [
    ["hann", t("audio.windows.hann")],
    ["hamming", t("audio.windows.hamming")],
    ["blackman", t("audio.windows.blackman")],
    ["rectangular", t("audio.windows.rectangular")],
  ];
  const downmixes: readonly (readonly [Downmix, string])[] = [
    ["average", t("audio.downmixes.average")],
    ["left", t("audio.downmixes.left")],
    ["right", t("audio.downmixes.right")],
    ["all_channels", t("audio.downmixes.all_channels")],
  ];

  return (
    <>
      <Section title={t("audio.inputTitle")} note={t("audio.inputNote")}>
        <DevicePicker value={audio.device} onChange={set("device")} onNotice={onNotice} />
      </Section>

      <Section title={t("audio.analysisTitle")} note={t("audio.analysisNote")}>
        <Field label={t("audio.fftSize")} note={t("audio.fftSizeNote")}>
          <select value={String(audio.fft_size)} onChange={(e) => set("fft_size")(Number(e.target.value))}>
            {(FFT_SIZES.includes(audio.fft_size) ? FFT_SIZES : [...FFT_SIZES, audio.fft_size].sort((a, b) => a - b)).map((n) => (
              <option key={n} value={n}>
                {n}
              </option>
            ))}
          </select>
        </Field>
        <NumberField
          label={t("audio.hopSize")}
          note={t("audio.hopSizeNote")}
          value={audio.hop_size}
          onChange={set("hop_size")}
          integer
          min={1}
          unit={t("audio.samples")}
        />
        <SelectField label={t("audio.window")} value={audio.window} onChange={set("window")} options={windows} />
        <SelectField label={t("audio.downmix")} note={t("audio.downmixNote")} value={audio.downmix} onChange={set("downmix")} options={downmixes} />
        <NumberField
          label={t("audio.buffer")}
          note={t("audio.bufferNote")}
          value={audio.buffer_duration_hns}
          onChange={set("buffer_duration_hns")}
          integer
          min={0}
          step={10000}
          unit={t("audio.bufferUnit")}
        />
      </Section>

      <Section title={t("audio.silenceTitle")} note={t("audio.silenceNote")}>
        <NumberField
          label={t("audio.silenceAfter")}
          note={t("audio.silenceAfterNote")}
          value={audio.silence_timeout_ms}
          onChange={set("silence_timeout_ms")}
          integer
          min={1}
          step={100}
          unit={t("units.ms")}
        />
        <NumberField
          label={t("audio.silenceFade")}
          value={audio.silence_fade_ms}
          onChange={set("silence_fade_ms")}
          integer
          min={0}
          step={100}
          unit={t("units.ms")}
        />
        <SliderField
          label={t("audio.silenceBrightness")}
          note={t("audio.silenceBrightnessNote")}
          value={audio.silence_brightness}
          onChange={set("silence_brightness")}
          min={0}
          max={1}
        />
      </Section>
    </>
  );
}

function DevicePicker({
  value,
  onChange,
  onNotice,
}: {
  value: AudioDeviceSelector;
  onChange: (next: AudioDeviceSelector) => void;
  onNotice: (n: Notice | null) => void;
}) {
  const { t } = useTranslation();
  const [devices, setDevices] = useState<AudioDeviceInfo[] | null>(null);
  const [loading, setLoading] = useState(false);

  const refresh = async () => {
    setLoading(true);
    try {
      setDevices(await api.listAudioDevices());
    } catch (e) {
      onNotice({ tone: "error", title: t("audio.deviceReadFailed", { error: asAppError(e).message }) });
      setDevices([]);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // The three selector shapes collapse to one dropdown: "default", or a concrete device addressed by its id.
  const selected = value.type === "id" ? value.id : value.type === "name" ? `name:${value.name}` : "default";

  return (
    <>
      <Field label={t("audio.device")} note={t("audio.deviceNote")}>
        <div className="inline">
          <select
            value={selected}
            disabled={loading}
            onChange={(e) => {
              const next = e.target.value;
              if (next === "default") onChange({ type: "default" });
              else if (next.startsWith("name:")) onChange({ type: "name", name: next.slice(5) });
              else onChange({ type: "id", id: next });
            }}
          >
            <option value="default">{t("audio.systemDefault")}</option>
            {/* A device named in the config but not found right now would otherwise silently vanish from the dropdown. */}
            {value.type === "name" && <option value={`name:${value.name}`}>{t("audio.byNameOption", { name: value.name })}</option>}
            {value.type === "id" && !devices?.some((d) => d.id === value.id) && (
              <option value={value.id}>{t("audio.unknownDevice", { id: value.id })}</option>
            )}
            {devices?.map((d) => (
              <option key={d.id} value={d.id}>
                {d.name}
                {d.is_default ? ` (${t("audio.systemDefault")})` : ""}
                {d.layout ? ` – ${d.layout}` : ""}
              </option>
            ))}
          </select>
          <button type="button" className="btn small" disabled={loading} onClick={() => void refresh()}>
            {loading ? "…" : t("audio.rescan")}
          </button>
        </div>
      </Field>

      <CheckboxField
        label={t("audio.byName")}
        note={t("audio.byNameNote")}
        text={t("audio.byNameText")}
        value={value.type === "name"}
        onChange={(on) => {
          if (on) {
            const current = devices?.find((d) => (value.type === "id" ? d.id === value.id : d.is_default));
            onChange({ type: "name", name: current?.name ?? "" });
          } else {
            onChange({ type: "default" });
          }
        }}
      />

      {value.type === "name" && (
        <Field label={t("audio.namePart")}>
          <input type="text" value={value.name} onChange={(e) => onChange({ type: "name", name: e.target.value })} />
        </Field>
      )}
    </>
  );
}
