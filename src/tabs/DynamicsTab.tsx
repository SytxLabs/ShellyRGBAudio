import { useTranslation } from "react-i18next";

import type { DynamicsSection, LevelSource, Normalize } from "../bindings";
import { ColorField, NumberField, Section, SelectField, SliderField } from "../components/fields";
import { patcher } from "../util";

export function DynamicsTab({ dynamics, onChange }: { dynamics: DynamicsSection; onChange: (next: DynamicsSection) => void }) {
  const { t } = useTranslation();
  const set = patcher(dynamics, onChange);

  const normalize: readonly (readonly [Normalize, string])[] = [
    ["shared", t("dynamics.normalize.shared")],
    ["per_band", t("dynamics.normalize.per_band")],
  ];
  const levels: readonly (readonly [LevelSource, string])[] = [
    ["peak", t("dynamics.levels.peak")],
    ["average", t("dynamics.levels.average")],
  ];

  return (
    <>
      <Section title={t("dynamics.normTitle")} note={t("dynamics.normNote")}>
        <SelectField
          label={t("dynamics.reference")}
          note={t("dynamics.referenceNote")}
          value={dynamics.normalize}
          onChange={set("normalize")}
          options={normalize}
        />
        <SelectField label={t("dynamics.levelSource")} value={dynamics.level_source} onChange={set("level_source")} options={levels} />
        <NumberField
          label={t("dynamics.logOffset")}
          note={t("dynamics.logOffsetNote")}
          value={dynamics.log_offset}
          onChange={set("log_offset")}
          min={0.001}
          step={0.1}
        />
        <NumberField
          label={t("dynamics.peakFloor")}
          note={t("dynamics.peakFloorNote")}
          value={dynamics.peak_floor}
          onChange={set("peak_floor")}
          min={0}
          step={0.000001}
        />
        <SliderField
          label={t("dynamics.peakDecay")}
          note={t("dynamics.peakDecayNote")}
          value={dynamics.peak_decay}
          onChange={set("peak_decay")}
          min={0.9}
          max={0.99999}
          step={0.0001}
        />
        <SliderField label={t("dynamics.levelAlpha")} value={dynamics.level_alpha} onChange={set("level_alpha")} min={0.01} max={1} />
      </Section>

      <Section title={t("dynamics.envTitle")} note={t("dynamics.envNote")}>
        <SliderField
          label={t("dynamics.attack")}
          note={t("dynamics.attackNote")}
          value={dynamics.band_attack}
          onChange={set("band_attack")}
          min={0.01}
          max={1}
        />
        <SliderField
          label={t("dynamics.release")}
          note={t("dynamics.releaseNote")}
          value={dynamics.band_release}
          onChange={set("band_release")}
          min={0.01}
          max={1}
        />
      </Section>

      <Section title={t("dynamics.beatTitle")} note={t("dynamics.beatNote")}>
        <SliderField label={t("dynamics.fluxAlpha")} value={dynamics.flux_alpha} onChange={set("flux_alpha")} min={0.01} max={1} />
        <SliderField
          label={t("dynamics.beatThreshold")}
          note={t("dynamics.beatThresholdNote")}
          value={dynamics.beat_threshold}
          onChange={set("beat_threshold")}
          min={0}
          max={0.999}
        />
        <NumberField
          label={t("dynamics.beatCooldown")}
          note={t("dynamics.beatCooldownNote")}
          value={dynamics.beat_cooldown_ms}
          onChange={set("beat_cooldown_ms")}
          integer
          min={0}
          step={10}
          unit={t("units.ms")}
        />
        <NumberField label={t("dynamics.strobeMs")} value={dynamics.strobe_ms} onChange={set("strobe_ms")} integer min={0} step={10} unit={t("units.ms")} />
        <SliderField label={t("dynamics.strobeLevel")} value={dynamics.strobe_level} onChange={set("strobe_level")} min={0} max={1} />
        <ColorField
          label={t("dynamics.strobeColor")}
          note={t("dynamics.strobeColorNote")}
          value={dynamics.strobe_color}
          onChange={set("strobe_color")}
          nullLabel={t("dynamics.ownColour")}
        />
      </Section>
    </>
  );
}
