import { useTranslation } from "react-i18next";

import type { BandConfig, ColorMapConfig, ColorStop, Interpolation, Scale, WhiteChannel } from "../bindings";
import { ColorField, ColorPicker, Field, NumberInput, Section, SelectField, SliderField, toSixDigit } from "../components/fields";
import { formatHz, patcher, removeAt, replaceAt } from "../util";

export function ColorsTab({
  bands,
  colorMap,
  onBands,
  onColorMap,
}: {
  bands: BandConfig[];
  colorMap: ColorMapConfig;
  onBands: (next: BandConfig[]) => void;
  onColorMap: (next: ColorMapConfig) => void;
}) {
  const { t } = useTranslation();
  const set = patcher(colorMap, onColorMap);

  const scales: readonly (readonly [Scale, string])[] = [
    ["log", t("colors.scales.log")],
    ["linear", t("colors.scales.linear")],
  ];
  const interpolations: readonly (readonly [Interpolation, string])[] = [
    ["srgb", t("colors.interpolations.srgb")],
    ["linear_light", t("colors.interpolations.linear_light")],
    ["hsv", t("colors.interpolations.hsv")],
  ];
  const white: readonly (readonly [WhiteChannel, string])[] = [
    ["off", t("colors.white.off")],
    ["min_channel", t("colors.white.min_channel")],
    ["from_color", t("colors.white.from_color")],
    ["fixed", t("colors.white.fixed")],
  ];

  return (
    <>
      <Section title={t("colors.bandsTitle")} note={t("colors.bandsNote")}>
        <table className="grid">
          <thead>
            <tr>
              <th style={{ width: "22%" }}>{t("colors.colName")}</th>
              <th style={{ width: 110 }}>{t("colors.colFrom")}</th>
              <th style={{ width: 110 }}>{t("colors.colTo")}</th>
              <th style={{ width: 90 }}>{t("colors.colWeight")}</th>
              <th>{t("colors.colColour")}</th>
              <th style={{ width: 90 }} />
            </tr>
          </thead>
          <tbody>
            {bands.map((band, i) => {
              const setBand = patcher(band, (next) => onBands(replaceAt(bands, i, next)));
              return (
                <tr key={i}>
                  <td>
                    <input type="text" value={band.name} onChange={(e) => setBand("name")(e.target.value)} />
                  </td>
                  <td>
                    <NumberInput value={band.from_hz} onChange={setBand("from_hz")} min={0} />
                  </td>
                  <td>
                    <NumberInput value={band.to_hz} onChange={setBand("to_hz")} min={0} />
                  </td>
                  <td>
                    <NumberInput value={band.weight} onChange={setBand("weight")} min={0} step={0.1} />
                  </td>
                  <td>
                    <div className="inline">
                      <input
                        type="checkbox"
                        checked={band.color !== null}
                        title={t("colors.ownColour")}
                        onChange={(e) => setBand("color")(e.target.checked ? "#FF0000" : null)}
                      />
                      {band.color !== null && <ColorPicker value={band.color} onChange={(v) => setBand("color")(v)} />}
                    </div>
                  </td>
                  <td>
                    <button type="button" className="btn small danger" onClick={() => onBands(removeAt(bands, i))}>
                      {t("colors.delete")}
                    </button>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>

        <div className="inline" style={{ marginTop: 10 }}>
          <button
            type="button"
            className="btn"
            onClick={() => {
              const last = bands[bands.length - 1];
              const from = last ? last.to_hz : 20;
              onBands([
                ...bands,
                { name: t("colors.bandDefaultName", { n: bands.length + 1 }), from_hz: from, to_hz: Math.min(from * 4, 20000), weight: 1, color: null },
              ]);
            }}
          >
            {t("colors.addBand")}
          </button>
          {bands.length === 0 && <span className="prose">{t("colors.noBands")}</span>}
        </div>
      </Section>

      <Section title={t("colors.mapTitle")} note={t("colors.mapNote")}>
        <div className="gradient" style={{ background: gradientCss(colorMap.stops, colorMap.frequency_scale) }} />
        <div className="gradient-scale">
          <span>{formatHz(20)}</span>
          <span>{formatHz(200)}</span>
          <span>{formatHz(2000)}</span>
          <span>{formatHz(20000)}</span>
        </div>

        <table className="grid">
          <thead>
            <tr>
              <th style={{ width: 140 }}>{t("colors.colHz")}</th>
              <th>{t("colors.colColour")}</th>
              <th style={{ width: 90 }} />
            </tr>
          </thead>
          <tbody>
            {colorMap.stops.map((stop, i) => {
              const setStop = patcher(stop, (next: ColorStop) => set("stops")(replaceAt(colorMap.stops, i, next)));
              return (
                <tr key={i}>
                  <td>
                    <NumberInput value={stop.hz} onChange={setStop("hz")} min={1} />
                  </td>
                  <td>
                    <ColorPicker value={stop.color} onChange={(v) => setStop("color")(v)} />
                  </td>
                  <td>
                    <button type="button" className="btn small danger" onClick={() => set("stops")(removeAt(colorMap.stops, i))}>
                      {t("colors.delete")}
                    </button>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>

        <div className="inline" style={{ marginTop: 10 }}>
          <button
            type="button"
            className="btn"
            onClick={() => set("stops")([...colorMap.stops, { hz: 1000, color: "#FFFFFF" }].sort((a, b) => a.hz - b.hz))}
          >
            {t("colors.addStop")}
          </button>
          <span className="prose">{t("colors.stopNote")}</span>
        </div>

        <div style={{ marginTop: 6 }}>
          <SelectField label={t("colors.scale")} value={colorMap.frequency_scale} onChange={set("frequency_scale")} options={scales} />
          <SelectField label={t("colors.interpolation")} value={colorMap.interpolation} onChange={set("interpolation")} options={interpolations} />
          <SliderField
            label={t("colors.saturation")}
            note={t("colors.saturationNote")}
            value={colorMap.saturation}
            onChange={set("saturation")}
            min={0}
            max={1}
          />
          <SliderField
            label={t("colors.valueFloor")}
            note={t("colors.valueFloorNote")}
            value={colorMap.value_floor}
            onChange={set("value_floor")}
            min={0}
            max={1}
          />
          <SliderField
            label={t("colors.valueSpan")}
            note={t("colors.valueSpanNote")}
            value={colorMap.value_span}
            onChange={set("value_span")}
            min={0}
            max={1}
          />
          <SelectField
            label={t("colors.whiteChannel")}
            note={t("colors.whiteChannelNote")}
            value={colorMap.white_channel}
            onChange={set("white_channel")}
            options={white}
          />
          {colorMap.white_channel === "fixed" && (
            <Field label={t("colors.whiteFixed")}>
              <div style={{ width: 130 }}>
                <NumberInput value={colorMap.white_fixed} onChange={set("white_fixed")} integer min={0} max={255} />
              </div>
            </Field>
          )}
          <ColorField
            label={t("colors.fallback")}
            note={t("colors.fallbackNote")}
            value={colorMap.fallback_color}
            onChange={(v) => set("fallback_color")(v ?? "#FF0000")}
          />
        </div>
      </Section>
    </>
  );
}

/** The same mapping the engine uses, drawn as a CSS gradient so the stops can be judged before saving. */
function gradientCss(stops: ColorStop[], scale: Scale): string {
  const lo = 20;
  const hi = 20000;
  const usable = stops.filter((s) => Number.isFinite(s.hz) && s.hz > 0).sort((a, b) => a.hz - b.hz);
  if (usable.length === 0) return "var(--surface-2)";
  if (usable.length === 1) return toSixDigit(usable[0]!.color);

  const at = (hz: number) => {
    const clamped = Math.min(Math.max(hz, lo), hi);
    const t = scale === "log" ? (Math.log(clamped) - Math.log(lo)) / (Math.log(hi) - Math.log(lo)) : (clamped - lo) / (hi - lo);
    return (t * 100).toFixed(1);
  };

  return `linear-gradient(90deg, ${usable.map((s) => `${toSixDigit(s.color)} ${at(s.hz)}%`).join(", ")})`;
}
