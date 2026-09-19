import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import type { Notice } from "../App";
import { api, asAppError } from "../api";
import type { DeviceEntry, DeviceTypeInfo, HueLightInfo } from "../bindings";
import { DeviceGeometryFields, GeometryHint } from "../components/SpatialFields";
import { CheckboxField, Field, NumberInput, Section, SelectField, TextField } from "../components/fields";
import { moveItem, removeAt, replaceAt } from "../util";

/**
 * Field descriptions rather than markup, so the four device types stay readable side by side and a new one is a table entry.
 *
 * Every `label`, `note`, `text` and option caption is a translation key, not a string — the schema is built once at module level and the language can
 * change under it.
 */
type FieldSpec =
  | { key: string; label: string; kind: "text"; note?: string; password?: boolean; placeholder?: string }
  | { key: string; label: string; kind: "number"; note?: string; min?: number; max?: number; step?: number; integer?: boolean; unit?: string }
  | { key: string; label: string; kind: "bool"; note?: string; text: string }
  | { key: string; label: string; kind: "select"; note?: string; options: readonly (readonly [string, string])[] }
  | { key: string; label: string; kind: "optnumber"; note?: string; integer?: boolean; min?: number; text: string; fallback: number };

const F = "devices.fields.";

const BRIGHTNESS: FieldSpec[] = [
  { key: "min_brightness", label: `${F}minBrightness`, kind: "number", integer: true, min: 0, max: 100, unit: "units.percent" },
  { key: "max_brightness", label: `${F}maxBrightness`, kind: "number", integer: true, min: 1, max: 100, unit: "units.percent" },
  { key: "brightness_gamma", label: `${F}gamma`, kind: "number", min: 0.01, step: 0.05, note: `${F}gammaNote` },
];

/** Mirrors the four `DeviceConfig` structs in `src-tauri/src/devices/`. A type missing here still works — it falls back to the raw JSON editor. */
const SCHEMAS: Record<string, { label: string; fields: FieldSpec[] }> = {
  shelly: {
    label: "devices.types.shelly",
    fields: [
      { key: "host", label: `${F}host`, kind: "text", note: `${F}hostNote`, placeholder: "192.168.1.50" },
      {
        key: "device",
        label: `${F}model`,
        kind: "select",
        note: `${F}modelNote`,
        options: [
          ["auto", `${F}shellyModels.auto`],
          ["rgbw2", `${F}shellyModels.rgbw2`],
          ["plus_rgbw_pm", `${F}shellyModels.plus_rgbw_pm`],
        ],
      },
      { key: "rgbw_id", label: `${F}channel`, kind: "number", integer: true, min: 0, note: `${F}channelNote` },
      ...BRIGHTNESS,
      { key: "http_timeout_ms", label: `${F}httpTimeout`, kind: "number", integer: true, min: 100, step: 100, unit: "units.ms" },
      { key: "gen2_min_transition_ms", label: `${F}gen2Min`, kind: "number", integer: true, min: 0, step: 50, unit: "units.ms" },
      { key: "gen2_max_transition_s", label: `${F}gen2Max`, kind: "number", min: 0, step: 10, unit: "units.s" },
    ],
  },
  govee_lan: {
    label: "devices.types.govee_lan",
    fields: [
      { key: "ip", label: `${F}ip`, kind: "text", placeholder: "192.168.1.60" },
      { key: "name", label: `${F}name`, kind: "text", note: `${F}nameNote` },
      ...BRIGHTNESS,
      { key: "dreamview", label: `${F}dreamview`, kind: "bool", text: `${F}dreamviewText`, note: `${F}dreamviewNote` },
      { key: "color_temp_kelvin", label: `${F}colorTemp`, kind: "number", integer: true, min: 0, note: `${F}colorTempNote`, unit: "units.kelvin" },
      { key: "remote_port", label: `${F}remotePort`, kind: "number", integer: true, min: 1, max: 65535 },
      { key: "local_bind_addr", label: `${F}localBind`, kind: "text", note: `${F}localBindNote` },
      { key: "local_port", label: `${F}localPort`, kind: "number", integer: true, min: 1, max: 65535, note: `${F}localPortNote` },
      { key: "read_timeout_ms", label: `${F}readTimeout`, kind: "number", integer: true, min: 10, step: 10, unit: "units.ms" },
    ],
  },
  govee_dreamview: {
    label: "devices.types.govee_dreamview",
    fields: [
      { key: "ip", label: `${F}ip`, kind: "text", placeholder: "192.168.1.61" },
      { key: "name", label: `${F}name`, kind: "text" },
      { key: "segments", label: `${F}segments`, kind: "number", integer: true, min: 1, max: 15, note: `${F}segmentsDreamNote` },
      { key: "reverse", label: `${F}reverse`, kind: "bool", text: `${F}reverseText` },
      ...BRIGHTNESS,
      { key: "color_temp_kelvin", label: `${F}colorTemp`, kind: "number", integer: true, min: 0, unit: "units.kelvin" },
      { key: "remote_port", label: `${F}remotePort`, kind: "number", integer: true, min: 1, max: 65535 },
      { key: "local_bind_addr", label: `${F}localBind`, kind: "text" },
      { key: "local_port", label: `${F}localPort`, kind: "number", integer: true, min: 1, max: 65535 },
      { key: "read_timeout_ms", label: `${F}readTimeout`, kind: "number", integer: true, min: 10, step: 10, unit: "units.ms" },
    ],
  },
  hue: {
    label: "devices.types.hue",
    fields: [
      { key: "bridge", label: `${F}bridge`, kind: "text", note: `${F}bridgeNote`, placeholder: "192.168.1.80" },
      { key: "application_key", label: `${F}applicationKey`, kind: "text", password: true, note: `${F}applicationKeyNote` },
      ...BRIGHTNESS,
      { key: "max_updates_per_second", label: `${F}hueRate`, kind: "number", min: 0.5, max: 20, step: 0.5, note: `${F}hueRateNote` },
      { key: "transitions", label: `${F}hueTransitions`, kind: "bool", text: `${F}hueTransitionsText`, note: `${F}hueTransitionsNote` },
      { key: "http_timeout_ms", label: `${F}httpTimeout`, kind: "number", integer: true, min: 100, step: 100, unit: "units.ms" },
      { key: "verify_tls", label: `${F}verifyTls`, kind: "bool", text: `${F}verifyTlsText`, note: `${F}verifyTlsNote` },
    ],
  },
  wled: {
    label: "devices.types.wled",
    fields: [
      { key: "host", label: `${F}host`, kind: "text", placeholder: "192.168.1.70" },
      { key: "leds", label: `${F}leds`, kind: "optnumber", text: `${F}ledsText`, fallback: 60, integer: true, min: 1, note: `${F}ledsNote` },
      { key: "segments", label: `${F}segments`, kind: "number", integer: true, min: 1, note: `${F}segmentsWledNote` },
      {
        key: "protocol",
        label: `${F}protocol`,
        kind: "select",
        note: `${F}protocolNote`,
        options: [
          ["dnrgb", "DNRGB"],
          ["drgb", "DRGB"],
          ["drgbw", "DRGBW"],
          ["warls", "WARLS"],
        ],
      },
      { key: "reverse", label: `${F}reverse`, kind: "bool", text: `${F}reverseText` },
      ...BRIGHTNESS,
      { key: "port", label: `${F}realtimePort`, kind: "number", integer: true, min: 1, max: 65535 },
      { key: "realtime_timeout_s", label: `${F}realtimeTimeout`, kind: "number", integer: true, min: 1, unit: "units.s", note: `${F}realtimeTimeoutNote` },
      { key: "http_timeout_ms", label: `${F}httpTimeout`, kind: "number", integer: true, min: 100, step: 100, unit: "units.ms" },
      { key: "local_bind_addr", label: `${F}localBind`, kind: "text" },
    ],
  },
};

export function DevicesTab({
  devices,
  onChange,
  onNotice,
}: {
  devices: DeviceEntry[];
  onChange: (next: DeviceEntry[]) => void;
  onNotice: (n: Notice | null) => void;
}) {
  const { t } = useTranslation();
  const [types, setTypes] = useState<DeviceTypeInfo[]>([]);
  const [adding, setAdding] = useState("");

  // `devices[]` is raw JSON, so a hand-written entry may simply not have a key — serde fills it in when the engine reads it. Showing 0 there would
  // both misinform and, on the next edit, write that 0 into the file. The registry's default entry is what the engine would actually use.
  const defaults = useMemo(() => Object.fromEntries(types.map((type) => [type.type_tag, type.default_entry as DeviceEntry])), [types]);

  useEffect(() => {
    void api
      .deviceTypes()
      .then((found) => {
        setTypes(found);
        setAdding(found[0]?.type_tag ?? "");
      })
      .catch((e) => onNotice({ tone: "error", title: asAppError(e).message }));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <>
      {devices.length === 0 && (
        <div className="notice warn">
          <h3>{t("devices.emptyTitle")}</h3>
          <p className="prose">{t("devices.emptyNote")}</p>
        </div>
      )}

      {devices.map((device, index) => (
        <DeviceCard
          key={index}
          device={device}
          index={index}
          count={devices.length}
          onChange={(next) => onChange(replaceAt(devices, index, next))}
          onRemove={() => onChange(removeAt(devices, index))}
          onMove={(delta) => onChange(moveItem(devices, index, index + delta))}
          defaults={defaults}
          onNotice={onNotice}
        />
      ))}

      <Section title={t("devices.addTitle")} note={t("devices.addNote")}>
        <div className="inline">
          <select value={adding} onChange={(e) => setAdding(e.target.value)}>
            {types.map((type) => (
              <option key={type.type_tag} value={type.type_tag}>
                {SCHEMAS[type.type_tag] ? t(SCHEMAS[type.type_tag]!.label) : type.type_tag}
              </option>
            ))}
          </select>
          <button
            type="button"
            className="btn"
            disabled={!adding}
            onClick={() => {
              const found = types.find((type) => type.type_tag === adding);
              if (found) onChange([...devices, structuredClone(found.default_entry) as DeviceEntry]);
            }}
          >
            {t("devices.add")}
          </button>
        </div>
      </Section>
    </>
  );
}

function DeviceCard({
  device,
  index,
  count,
  onChange,
  onRemove,
  onMove,
  defaults,
  onNotice,
}: {
  device: DeviceEntry;
  index: number;
  count: number;
  onChange: (next: DeviceEntry) => void;
  onRemove: () => void;
  onMove: (delta: number) => void;
  defaults: Record<string, DeviceEntry>;
  onNotice: (n: Notice | null) => void;
}) {
  const { t } = useTranslation();
  const [testing, setTesting] = useState(false);
  const [raw, setRaw] = useState(false);
  const tag = typeof device.type === "string" ? device.type : "";
  const schema = SCHEMAS[tag];
  const set = (key: string) => (value: unknown) => onChange({ ...device, [key]: value });

  const label = String(device.name ?? device.host ?? device.ip ?? tag ?? "");

  const test = async () => {
    setTesting(true);
    try {
      const name = await api.testDevice(device);
      onNotice({ tone: "ok", title: t("devices.testOk", { name }), lines: [t("devices.testOkNote")] });
    } catch (e) {
      onNotice({ tone: "error", title: t("devices.testFailed", { error: asAppError(e).message }) });
    } finally {
      setTesting(false);
    }
  };

  return (
    <div className="card">
      <header>
        <span className="tag">{schema ? t(schema.label) : tag}</span>
        <span className="title">{label}</span>
        <span className="spacer" />
        <button type="button" className="btn small" disabled={index === 0} onClick={() => onMove(-1)} title={t("devices.moveUp")}>
          ↑
        </button>
        <button type="button" className="btn small" disabled={index === count - 1} onClick={() => onMove(1)} title={t("devices.moveDown")}>
          ↓
        </button>
        <button type="button" className="btn small" onClick={() => setRaw((v) => !v)}>
          {raw ? t("devices.form") : t("devices.json")}
        </button>
        <button type="button" className="btn small" disabled={testing} onClick={() => void test()}>
          {testing ? t("devices.testing") : t("devices.test")}
        </button>
        <button type="button" className="btn small danger" onClick={onRemove}>
          {t("devices.remove")}
        </button>
      </header>

      <div className="body">
        {raw || !schema ? (
          <>
            {!schema && <p className="prose">{t("devices.noSchema", { type: tag })}</p>}
            <RawEditor device={device} onChange={onChange} />
          </>
        ) : (
          <>
            {schema.fields.map((spec) => (
              <DeviceField key={spec.key} spec={spec} value={device[spec.key] ?? defaults[tag]?.[spec.key]} onChange={set(spec.key)} />
            ))}
            {tag === "shelly" && <ShellyAuth device={device} onChange={onChange} />}
            {tag === "hue" && <HueLightPicker device={device} onChange={onChange} onNotice={onNotice} />}
            <details className="geometry">
              <summary>{t("geometry.summary")}</summary>
              <DeviceGeometryFields device={device} onChange={onChange} />
              <GeometryHint />
            </details>
          </>
        )}
      </div>
    </div>
  );
}

function DeviceField({ spec, value, onChange }: { spec: FieldSpec; value: unknown; onChange: (v: unknown) => void }) {
  const { t } = useTranslation();

  switch (spec.kind) {
    case "text":
      return (
        <TextField
          label={t(spec.label)}
          note={spec.note ? t(spec.note) : undefined}
          placeholder={spec.placeholder}
          password={spec.password}
          value={typeof value === "string" ? value : ""}
          onChange={onChange}
        />
      );
    case "number":
      return (
        <Field label={t(spec.label)} note={spec.note ? t(spec.note) : undefined}>
          <div className="inline">
            <div style={{ width: 130 }}>
              <NumberInput
                value={typeof value === "number" ? value : 0}
                onChange={onChange}
                min={spec.min}
                max={spec.max}
                step={spec.step}
                integer={spec.integer}
              />
            </div>
            {spec.unit && <span className="prose">{t(spec.unit)}</span>}
          </div>
        </Field>
      );
    case "bool":
      return (
        <CheckboxField
          label={t(spec.label)}
          note={spec.note ? t(spec.note) : undefined}
          text={t(spec.text)}
          value={value === true}
          onChange={onChange}
        />
      );
    case "select":
      return (
        <SelectField
          label={t(spec.label)}
          note={spec.note ? t(spec.note) : undefined}
          value={typeof value === "string" ? value : (spec.options[0]?.[0] ?? "")}
          onChange={onChange}
          // Protocol names like DNRGB are not translated and pass through t() untouched, so both kinds of caption work here.
          options={spec.options.map(([v, caption]) => [v, caption.includes(".") ? t(caption) : caption] as const)}
        />
      );
    case "optnumber":
      return (
        <Field label={t(spec.label)} note={spec.note ? t(spec.note) : undefined}>
          <div className="inline">
            <label className="inline" style={{ cursor: "pointer" }}>
              <input
                type="checkbox"
                checked={value !== null && value !== undefined}
                onChange={(e) => onChange(e.target.checked ? spec.fallback : null)}
              />
              <span className="prose">{t(spec.text)}</span>
            </label>
            {typeof value === "number" && (
              <div style={{ width: 130 }}>
                <NumberInput value={value} onChange={onChange} min={spec.min} integer={spec.integer} />
              </div>
            )}
          </div>
        </Field>
      );
  }
}

function ShellyAuth({ device, onChange }: { device: DeviceEntry; onChange: (next: DeviceEntry) => void }) {
  const { t } = useTranslation();
  const auth = (device.auth ?? null) as { username?: string; password?: string } | null;

  return (
    <>
      <CheckboxField
        label={t(`${F}auth`)}
        text={t(`${F}authText`)}
        note={t(`${F}authNote`)}
        value={auth !== null}
        onChange={(on) => onChange({ ...device, auth: on ? { username: "admin", password: "" } : null })}
      />
      {auth !== null && (
        <>
          <TextField label={t(`${F}username`)} value={auth.username ?? ""} onChange={(v) => onChange({ ...device, auth: { ...auth, username: v } })} />
          <TextField
            label={t(`${F}password`)}
            password
            value={auth.password ?? ""}
            onChange={(v) => onChange({ ...device, auth: { ...auth, password: v } })}
          />
        </>
      )}
    </>
  );
}

/**
 * The lights of one Hue bridge, picked from a list rather than typed as resource ids.
 *
 * The order is what makes a row of Hue lamps a strip: with a gradient the engine hands out one color per entry, in the order they stand here, so the
 * list can be sorted to match how the lamps actually stand in the room.
 */
function HueLightPicker({
  device,
  onChange,
  onNotice,
}: {
  device: DeviceEntry;
  onChange: (next: DeviceEntry) => void;
  onNotice: (n: Notice | null) => void;
}) {
  const { t } = useTranslation();
  const [found, setFound] = useState<HueLightInfo[] | null>(null);
  const [busy, setBusy] = useState<"pair" | "scan" | null>(null);

  const bridge = typeof device.bridge === "string" ? device.bridge : "";
  const key = typeof device.application_key === "string" ? device.application_key : "";
  const selected = Array.isArray(device.lights) ? device.lights.filter((id): id is string => typeof id === "string") : [];
  const setLights = (lights: string[]) => onChange({ ...device, lights });

  const nameOf = (id: string) => found?.find((light) => light.id === id)?.name ?? id;

  const pair = async () => {
    setBusy("pair");
    try {
      const applicationKey = await api.huePair(bridge);
      onChange({ ...device, application_key: applicationKey });
      onNotice({ tone: "ok", title: t(`${F}huePairOk`) });
    } catch (e) {
      onNotice({ tone: "error", title: t(`${F}huePairFailed`, { error: asAppError(e).message }) });
    } finally {
      setBusy(null);
    }
  };

  const scan = async () => {
    setBusy("scan");
    try {
      setFound(await api.hueLights(bridge, key));
    } catch (e) {
      onNotice({ tone: "error", title: t(`${F}hueScanFailed`, { error: asAppError(e).message }) });
    } finally {
      setBusy(null);
    }
  };

  return (
    <>
      <Field label={t(`${F}hueBridgeActions`)} note={t(`${F}hueBridgeActionsNote`)}>
        <div className="inline">
          <button type="button" className="btn small" disabled={busy !== null || bridge === ""} onClick={() => void pair()}>
            {busy === "pair" ? t(`${F}huePairing`) : t(`${F}huePair`)}
          </button>
          <button type="button" className="btn small" disabled={busy !== null || bridge === "" || key === ""} onClick={() => void scan()}>
            {busy === "scan" ? t(`${F}hueScanning`) : t(`${F}hueScan`)}
          </button>
        </div>
      </Field>

      {found !== null && (
        <Field label={t(`${F}hueFound`)} note={t(`${F}hueFoundNote`)}>
          {found.length === 0 ? (
            <p className="prose">{t(`${F}hueNoLights`)}</p>
          ) : (
            <div className="device-chips">
              {found.map((light) => (
                <button
                  key={light.id}
                  type="button"
                  className={`chip ${selected.includes(light.id) ? "on" : ""}`}
                  onClick={() => setLights(selected.includes(light.id) ? selected.filter((id) => id !== light.id) : [...selected, light.id])}
                >
                  {light.name}
                </button>
              ))}
            </div>
          )}
        </Field>
      )}

      <Field label={t(`${F}hueSelected`)} note={t(`${F}hueSelectedNote`)}>
        {selected.length === 0 ? (
          <p className="prose">{t(`${F}hueNoneSelected`)}</p>
        ) : (
          <table className="grid">
            <tbody>
              {selected.map((id, i) => (
                <tr key={id}>
                  <td className="num">{i + 1}</td>
                  <td>{nameOf(id)}</td>
                  <td style={{ width: 130 }}>
                    <div className="inline">
                      <button type="button" className="btn small" disabled={i === 0} onClick={() => setLights(moveItem(selected, i, i - 1))}>
                        ↑
                      </button>
                      <button
                        type="button"
                        className="btn small"
                        disabled={i === selected.length - 1}
                        onClick={() => setLights(moveItem(selected, i, i + 1))}
                      >
                        ↓
                      </button>
                      <button type="button" className="btn small danger" onClick={() => setLights(removeAt(selected, i))}>
                        {t("devices.remove")}
                      </button>
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </Field>
    </>
  );
}

function RawEditor({ device, onChange }: { device: DeviceEntry; onChange: (next: DeviceEntry) => void }) {
  const { t } = useTranslation();
  const [text, setText] = useState(() => JSON.stringify(device, null, 2));
  const [error, setError] = useState<string | null>(null);

  return (
    <Field wide note={error ?? undefined}>
      <textarea
        spellCheck={false}
        value={text}
        onChange={(e) => {
          setText(e.target.value);
          try {
            const parsed: unknown = JSON.parse(e.target.value);
            if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) throw new Error(t("devices.jsonObject"));
            setError(null);
            onChange(parsed as DeviceEntry);
          } catch (e) {
            setError(e instanceof Error ? e.message : String(e));
          }
        }}
      />
    </Field>
  );
}
