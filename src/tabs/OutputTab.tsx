import { useTranslation } from "react-i18next";

import type { OutputSection } from "../bindings";
import { NumberField, Section, SliderField } from "../components/fields";
import { patcher } from "../util";

export function OutputTab({ output, onChange }: { output: OutputSection; onChange: (next: OutputSection) => void }) {
  const { t } = useTranslation();
  const set = patcher(output, onChange);

  return (
    <>
      <Section title={t("output.rateTitle")} note={t("output.rateNote")}>
        <NumberField
          label={t("output.interval")}
          note={t("output.intervalNote")}
          value={output.change_interval_ms}
          onChange={set("change_interval_ms")}
          integer
          min={0}
          step={10}
          unit={t("units.ms")}
        />
        <NumberField
          label={t("output.transitionMin")}
          value={output.transition_min_ms}
          onChange={set("transition_min_ms")}
          integer
          min={0}
          step={10}
          unit={t("units.ms")}
        />
        <NumberField
          label={t("output.transitionMax")}
          value={output.transition_max_ms}
          onChange={set("transition_max_ms")}
          integer
          min={0}
          step={10}
          unit={t("units.ms")}
        />
        <SliderField
          label={t("output.beatWeight")}
          note={t("output.beatWeightNote")}
          value={output.transition_beat_weight}
          onChange={set("transition_beat_weight")}
          min={0}
          max={1}
        />
        <SliderField label={t("output.levelWeight")} value={output.transition_level_weight} onChange={set("transition_level_weight")} min={0} max={1} />
        <NumberField
          label={t("output.curve")}
          note={t("output.curveNote")}
          value={output.transition_curve}
          onChange={set("transition_curve")}
          min={0.1}
          step={0.1}
        />
      </Section>

      <Section title={t("output.deadbandTitle")} note={t("output.deadbandNote")}>
        <NumberField
          label={t("output.deadbandRgb")}
          note={t("output.deadbandRgbNote")}
          value={output.deadband_rgb}
          onChange={set("deadband_rgb")}
          integer
          min={0}
          max={255}
        />
        <SliderField label={t("output.deadbandOverall")} value={output.deadband_overall} onChange={set("deadband_overall")} min={0} max={0.5} step={0.005} />
      </Section>

      <Section title={t("output.limitsTitle")} note={t("output.limitsNote")}>
        <NumberField
          label={t("output.floor")}
          note={t("output.floorNote")}
          value={output.brightness_floor}
          onChange={set("brightness_floor")}
          integer
          min={0}
          max={100}
          unit={t("units.percent")}
        />
        <NumberField label={t("output.gammaMin")} value={output.gamma_min} onChange={set("gamma_min")} min={0.01} step={0.1} />
        <NumberField label={t("output.gammaMax")} value={output.gamma_max} onChange={set("gamma_max")} min={0.1} step={0.1} />
      </Section>
    </>
  );
}
