import { Trans, useTranslation } from "react-i18next";

import type { DeviceEntry } from "../bindings";
import {
  deviceExtent,
  devicePath,
  devicePosition,
  extentToShape,
  isStrip,
  legCount,
  legShape,
  setLegShape,
  pathFromStraight,
  pathLength,
  pathWithCorner,
  shapeToExtent,
  straightFromPath,
  type StripShape,
  type Vec3,
} from "../geometry";
import { removeAt, replaceAt } from "../util";
import { CheckboxField, Field, NumberInput, SelectField, SliderField } from "./fields";

export function Vec3Field({
  label,
  note,
  value,
  onChange,
  step = 0.1,
  disabled,
}: {
  label: string;
  note?: string;
  value: Vec3;
  onChange: (v: Vec3) => void;
  step?: number;
  disabled?: boolean;
}) {
  const { t } = useTranslation();
  const axis = (i: 0 | 1 | 2) => (n: number) => {
    const next: Vec3 = [...value];
    next[i] = n;
    onChange(next);
  };

  return (
    <Field label={label} note={note}>
      <div className="axes">
        {(["X", "Y", "Z"] as const).map((name, i) => (
          <label key={name} className="axis">
            <span>{name}</span>
            <NumberInput value={value[i] ?? 0} onChange={axis(i as 0 | 1 | 2)} step={step} disabled={disabled} />
          </label>
        ))}
        <span className="prose">{t("units.metres")}</span>
      </div>
    </Field>
  );
}

/**
 * The geometry of one device, as the forms show it.
 *
 * A strip is stated as a length and two angles rather than the half-vector the config stores, because nobody thinks about a light strip as
 * `[0, 1.2, 0]` — they think "1.4 metres, running up the wall". A strip that runs around a corner has no single length and no single direction, so
 * it drops both and is stated as the corner list it passes through.
 */
export function DeviceGeometryFields({ device, onChange }: { device: DeviceEntry; onChange: (next: DeviceEntry) => void }) {
  const { t } = useTranslation();
  const set = (key: string) => (value: unknown) => onChange({ ...device, [key]: value });
  const position = devicePosition(device);
  const extent = deviceExtent(device);
  const strip = isStrip(device);
  const shape = extentToShape(extent);
  const spatiality = typeof device.spatiality === "number" ? device.spatiality : 0;
  const corners = devicePath(device);
  const bent = corners !== null;

  const setShape = (patch: Partial<typeof shape>) => set("extent")(shapeToExtent({ ...shape, ...patch }));
  // A corner list and a centre with a half-vector describe the same strip in two ways, so only ever one of them is stored.
  const setPath = (path: Vec3[] | null) =>
    onChange(path === null ? { ...device, path: null, ...straightFromPath(corners ?? []) } : { ...device, form: "strip", path, position: null, extent: [0, 0, 0] });

  return (
    <>
      <SelectField
        label={t("geometry.form")}
        value={strip ? "strip" : "lamp"}
        onChange={(form) =>
          onChange({
            ...device,
            form,
            path: null,
            extent: form === "strip" ? shapeToExtent({ length: 1, yaw: 0, pitch: 90 }) : [0, 0, 0],
            position: position ?? (bent ? straightFromPath(corners ?? []).position : null),
          })
        }
        options={[
          ["lamp", t("geometry.forms.lamp")],
          ["strip", t("geometry.forms.strip")],
        ]}
      />

      {bent ? (
        <>
          <Field label={t("geometry.path")} note={t("geometry.pathNote")}>
            <div className="inline">
              <span className="tag">{t("geometry.corners", { count: corners.length })}</span>
              <span className="prose">{t("geometry.pathLength", { metres: pathLength(corners).toFixed(2) })}</span>
            </div>
          </Field>

          {/* One length and one pair of angles per leg: a bent strip does not have a single direction, it has one for every run between two corners. */}
          {Array.from({ length: legCount(corners) }, (_, i) => {
            const leg = legShape(corners, i);
            const setLeg = (patch: Partial<StripShape>) => setPath(setLegShape(corners, i, { ...leg, ...patch }));
            return (
              <Field key={`leg${i}`} label={t("geometry.leg", { n: i + 1 })} note={i === 0 ? t("geometry.legNote") : undefined}>
                <div className="axes">
                  <label className="axis">
                    <span>{t("geometry.length")}</span>
                    <NumberInput value={leg.length} onChange={(length) => setLeg({ length })} min={0} step={0.1} />
                  </label>
                  <label className="axis">
                    <span>{t("geometry.yaw")}</span>
                    <NumberInput value={leg.yaw} onChange={(yaw) => setLeg({ yaw })} min={-180} max={180} step={5} />
                  </label>
                  <label className="axis">
                    <span>{t("geometry.pitch")}</span>
                    <NumberInput value={leg.pitch} onChange={(pitch) => setLeg({ pitch })} min={-90} max={90} step={5} />
                  </label>
                </div>
              </Field>
            );
          })}

          {corners.map((corner, i) => (
            <Field key={i} label={t("geometry.corner", { n: i + 1 })}>
              <div className="axes">
                {(["X", "Y", "Z"] as const).map((name, axis) => (
                  <label key={name} className="axis">
                    <span>{name}</span>
                    <NumberInput
                      value={corner[axis] ?? 0}
                      onChange={(n) => {
                        const next: Vec3 = [...corner];
                        next[axis as 0 | 1 | 2] = n;
                        setPath(replaceAt(corners, i, next));
                      }}
                      step={0.1}
                    />
                  </label>
                ))}
                <span className="prose">{t("units.metres")}</span>
                <button type="button" className="btn small danger" disabled={corners.length <= 2} onClick={() => setPath(removeAt(corners, i))}>
                  {t("geometry.removeCorner")}
                </button>
              </div>
            </Field>
          ))}

          <Field label={t("geometry.pathEdit")} note={t("geometry.pathEditNote")}>
            <div className="inline">
              <button type="button" className="btn small" onClick={() => setPath(pathWithCorner(corners))}>
                {t("geometry.addCorner")}
              </button>
              <button type="button" className="btn small" onClick={() => setPath(null)}>
                {t("geometry.straighten")}
              </button>
            </div>
          </Field>
        </>
      ) : (
        strip && (
          <>
            <Field label={t("geometry.length")} note={t("geometry.lengthNote")}>
              <div className="inline">
                <div style={{ width: 130 }}>
                  <NumberInput value={shape.length} onChange={(length) => setShape({ length })} min={0} step={0.1} />
                </div>
                <span className="prose">{t("units.metres")}</span>
              </div>
            </Field>
            <Field label={t("geometry.orientation")} note={t("geometry.orientationNote")}>
              <div className="axes">
                <label className="axis">
                  <span>{t("geometry.yaw")}</span>
                  <NumberInput value={shape.yaw} onChange={(yaw) => setShape({ yaw })} min={-180} max={180} step={5} />
                </label>
                <label className="axis">
                  <span>{t("geometry.pitch")}</span>
                  <NumberInput value={shape.pitch} onChange={(pitch) => setShape({ pitch })} min={-90} max={90} step={5} />
                </label>
                <span className="prose">{t("units.degrees")}</span>
              </div>
            </Field>
            <Field label={t("geometry.bend")} note={t("geometry.bendNote")}>
              <button type="button" className="btn small" onClick={() => setPath(pathFromStraight(position ?? [0, 1, 0], extent))}>
                {t("geometry.addCorner")}
              </button>
            </Field>
          </>
        )
      )}

      {/* A bent strip carries its place in the room in the corner list itself, so the position switch would have nothing left to say. */}
      {!bent && (
        <CheckboxField
          label={t("geometry.positioned")}
          text={t("geometry.positionedText")}
          note={t("geometry.positionedNote")}
          value={position !== null}
          onChange={(on) => onChange({ ...device, position: on ? [0, 1, 0] : null })}
        />
      )}

      {!bent && position !== null && (
        <Vec3Field label={strip ? t("geometry.centre") : t("geometry.position")} value={position} onChange={(v) => set("position")(v)} />
      )}

      {(bent || position !== null) && (
        <SliderField
          label={t("geometry.spatiality")}
          note={t("geometry.spatialityNote")}
          value={spatiality}
          onChange={set("spatiality")}
          min={0}
          max={1}
        />
      )}
    </>
  );
}

/** The pointer to the 3D view and the JSON-only settings, kept here so both places that show the geometry say the same thing. */
export function GeometryHint() {
  return (
    <p className="prose" style={{ margin: "4px 0 10px" }}>
      <Trans i18nKey="geometry.advancedHint" components={{ b: <b />, code: <code /> }} />
    </p>
  );
}
