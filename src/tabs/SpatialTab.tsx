import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { api } from "../api";
import type { DeviceEntry, EngineStatus, SpatialSection, SpeakerPlacement, SpeakerRole } from "../bindings";
import { LazyRoomView } from "../components/LazyRoomView";
import type { TransformMode } from "../components/RoomView";
import { DeviceGeometryFields, Vec3Field } from "../components/SpatialFields";
import { CheckboxField, NumberField, Section, SliderField } from "../components/fields";
import {
  deviceExtent,
  deviceLabel,
  devicePath,
  devicePosition,
  extentFromDirection,
  isPositioned,
  isStrip,
  len,
  pathFromStraight,
  pathWithCorner,
  type Vec3,
} from "../geometry";
import { patcher, removeAt, replaceAt } from "../util";

const ROLES: readonly SpeakerRole[] = [
  "front_left",
  "front_right",
  "front_center",
  "lfe",
  "back_left",
  "back_right",
  "side_left",
  "side_right",
  "front_left_of_center",
  "front_right_of_center",
  "back_center",
  "top_center",
  "top_front_left",
  "top_front_center",
  "top_front_right",
  "top_back_left",
  "top_back_center",
  "top_back_right",
  "unknown",
];

export function SpatialTab({
  spatial,
  onChange,
  devices,
  onDevices,
  status,
}: {
  spatial: SpatialSection;
  onChange: (next: SpatialSection) => void;
  devices: DeviceEntry[];
  onDevices: (next: DeviceEntry[]) => void;
  status: EngineStatus | null;
}) {
  const { t } = useTranslation();
  const set = patcher(spatial, onChange);
  const layout = spatial.layout;
  const manual = layout !== "auto";
  const channels: SpeakerRole[] = layout === "auto" ? [] : layout.channels;

  const [fallback, setFallback] = useState<SpeakerPlacement[]>([]);
  const [selected, setSelected] = useState<number | null>(null);
  const [mode, setMode] = useState<TransformMode>("translate");

  // The running engine knows the real layout. While it is stopped, ask what the current setting would produce so the view still has a reference frame.
  const live = status?.speakers ?? [];
  useEffect(() => {
    if (live.length > 0) return;
    let cancelled = false;
    void api
      .resolveSpeakers(spatial.layout, status?.channels ?? undefined)
      .then((found) => {
        if (!cancelled) setFallback(found);
      })
      .catch(() => setFallback([]));
    return () => {
      cancelled = true;
    };
  }, [live.length, JSON.stringify(spatial.layout), status?.channels]);

  const speakers = live.length > 0 ? live : fallback;
  const positioned = useMemo(() => devices.filter((device) => isPositioned(device)), [devices]);
  const current = selected === null ? undefined : devices[selected];

  const patchDevice = (index: number, patch: Partial<DeviceEntry>) =>
    onDevices(replaceAt(devices, index, { ...devices[index], ...patch } as DeviceEntry));

  /**
   * Bends the selected strip: a straight one becomes a corner list of its own two ends plus one leg around the corner, a bent one gets one corner
   * more. The corner list replaces `position` and `extent`, which only describe a straight line.
   */
  const addCorner = (index: number) => {
    const device = devices[index];
    if (!device) return;
    const existing = devicePath(device);
    const path: Vec3[] = existing ? pathWithCorner(existing) : pathFromStraight(devicePosition(device) ?? [0, 1, 0], deviceExtent(device));
    patchDevice(index, { form: "strip", path, position: null, extent: [0, 0, 0] });
  };

  const bendable = current !== undefined && isStrip(current);

  return (
    <>
      <Section title={t("spatial.viewTitle")} note={t("spatial.viewNote")}>
        <div className="roomview-toolbar">
          <div className="segmented">
            <button type="button" className={mode === "translate" ? "on" : ""} onClick={() => setMode("translate")}>
              {t("spatial.translate")}
            </button>
            <button type="button" className={mode === "rotate" ? "on" : ""} disabled={!current || !isStrip(current)} onClick={() => setMode("rotate")}>
              {t("spatial.rotate")}
            </button>
          </div>
          <button type="button" className="btn small" disabled={!bendable} onClick={() => selected !== null && addCorner(selected)}>
            {t("spatial.addCorner")}
          </button>
          <span className="spacer" />
          <span className="prose">
            {positioned.length === 0 ? t("spatial.nonePositioned") : t("spatial.somePositioned", { count: positioned.length, total: devices.length })}
          </span>
        </div>

        <LazyRoomView
          room={spatial.room}
          speakers={speakers}
          devices={devices}
          selected={selected}
          mode={mode}
          onSelect={setSelected}
          onMove={(index, position) => patchDevice(index, { position })}
          onOrient={(index, direction) => {
            const device = devices[index];
            if (!device) return;
            patchDevice(index, { extent: extentFromDirection(direction, len(deviceExtent(device)) * 2) });
          }}
          onPath={(index, path) => patchDevice(index, { path })}
        />

        {!spatial.enabled && (
          <p className="prose" style={{ marginTop: 10 }}>
            {t("spatial.disabledHint")}
          </p>
        )}

        <div className="device-chips">
          {devices.map((device, index) => (
            <button
              key={index}
              type="button"
              className={`chip ${index === selected ? "on" : ""} ${isPositioned(device) ? "" : "faded"}`}
              onClick={() => setSelected(index === selected ? null : index)}
            >
              {deviceLabel(device)}
              {!isPositioned(device) && <span className="prose"> · {t("spatial.noPosition")}</span>}
            </button>
          ))}
        </div>
      </Section>

      {current && (
        <Section title={t("spatial.geometryTitle", { name: deviceLabel(current) })} note={t("spatial.geometryNote")}>
          <DeviceGeometryFields device={current} onChange={(next) => selected !== null && onDevices(replaceAt(devices, selected, next))} />
        </Section>
      )}

      <Section title={t("spatial.distributionTitle")} note={t("spatial.distributionNote")}>
        <CheckboxField label={t("spatial.enabled")} text={t("spatial.enabledText")} value={spatial.enabled} onChange={set("enabled")} />
        <SliderField
          label={t("spatial.focus")}
          note={t("spatial.focusNote")}
          value={spatial.focus}
          onChange={set("focus")}
          min={0.1}
          max={16}
          step={0.1}
          disabled={!spatial.enabled}
        />
        <SliderField
          label={t("spatial.omniFloor")}
          note={t("spatial.omniFloorNote")}
          value={spatial.omni_floor}
          onChange={set("omni_floor")}
          min={0}
          max={1}
          disabled={!spatial.enabled}
        />
        <SliderField
          label={t("spatial.heightSharpness")}
          note={t("spatial.heightSharpnessNote")}
          value={spatial.height_sharpness}
          onChange={set("height_sharpness")}
          min={0.05}
          max={4}
          step={0.05}
          disabled={!spatial.enabled}
        />
        <SliderField
          label={t("spatial.distanceFalloff")}
          note={t("spatial.distanceFalloffNote")}
          value={spatial.distance_falloff}
          onChange={set("distance_falloff")}
          min={0}
          max={2}
          step={0.05}
          disabled={!spatial.enabled}
        />
        <NumberField
          label={t("spatial.stripSamples")}
          note={t("spatial.stripSamplesNote")}
          value={spatial.strip_samples}
          onChange={set("strip_samples")}
          integer
          min={1}
          max={256}
          disabled={!spatial.enabled}
        />
      </Section>

      <Section title={t("spatial.roomTitle")} note={t("spatial.roomNote")}>
        <Vec3Field label={t("spatial.roomMin")} value={spatial.room.min} onChange={(v) => set("room")({ ...spatial.room, min: v })} />
        <Vec3Field label={t("spatial.roomMax")} value={spatial.room.max} onChange={(v) => set("room")({ ...spatial.room, max: v })} />
      </Section>

      <Section title={t("spatial.layoutTitle")} note={t("spatial.layoutNote")}>
        <CheckboxField
          label={t("spatial.layout")}
          text={t("spatial.layoutManual")}
          value={manual}
          onChange={(on) => set("layout")(on ? { channels: ["front_left", "front_right"] } : "auto")}
        />

        {manual && (
          <>
            <table className="grid">
              <thead>
                <tr>
                  <th style={{ width: 80 }}>{t("spatial.colChannel")}</th>
                  <th>{t("spatial.colSpeaker")}</th>
                  <th style={{ width: 90 }} />
                </tr>
              </thead>
              <tbody>
                {channels.map((role, i) => (
                  <tr key={i}>
                    <td className="num">{i}</td>
                    <td>
                      <select value={role} onChange={(e) => set("layout")({ channels: replaceAt(channels, i, e.target.value as SpeakerRole) })}>
                        {ROLES.map((r) => (
                          <option key={r} value={r}>
                            {t(`speakers.${r}`)}
                          </option>
                        ))}
                      </select>
                    </td>
                    <td>
                      <button type="button" className="btn small danger" onClick={() => set("layout")({ channels: removeAt(channels, i) })}>
                        {t("spatial.delete")}
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
            <div className="inline" style={{ marginTop: 10 }}>
              <button type="button" className="btn" onClick={() => set("layout")({ channels: [...channels, "unknown"] })}>
                {t("spatial.addChannel")}
              </button>
            </div>
          </>
        )}
      </Section>
    </>
  );
}
