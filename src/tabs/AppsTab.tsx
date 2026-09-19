import { useState } from "react";
import { useTranslation } from "react-i18next";

import type { Notice } from "../App";
import { api, asAppError } from "../api";
import type { AppGroup, AppInfo, AppMatchMode, AudioSection } from "../bindings";
import { CheckboxField, Field, NumberField, NumberInput, Section, SelectField } from "../components/fields";
import { patcher, removeAt, replaceAt } from "../util";

export function AppsTab({
  audio,
  onChange,
  onNotice,
}: {
  audio: AudioSection;
  onChange: (next: AudioSection) => void;
  onNotice: (n: Notice | null) => void;
}) {
  const { t } = useTranslation();
  const apps = audio.apps;
  const set = patcher(apps, (next) => onChange({ ...audio, apps: next }));
  const [found, setFound] = useState<AppInfo[] | null>(null);
  const [loading, setLoading] = useState(false);

  const modes: readonly (readonly [AppMatchMode, string])[] = [["include", t("apps.modes.include")], ["exclude", t("apps.modes.exclude")]];

  const scan = async () => {
    setLoading(true);
    try {
      setFound(await api.listAudioApps());
    } catch (e) {
      onNotice({ tone: "error", title: t("apps.appsReadFailed", { error: asAppError(e).message }) });
    } finally {
      setLoading(false);
    }
  };

  const setGroups = (groups: AppGroup[]) => set("groups")(groups);

  return (
    <>
      <Section title={t("apps.title")} note={t("apps.note")}>
        <CheckboxField label={t("apps.enabled")} text={t("apps.enabledText")} value={apps.enabled} onChange={set("enabled")} />
        <SelectField label={t("apps.mode")} value={apps.mode} onChange={set("mode")} options={modes} disabled={!apps.enabled} />
        <CheckboxField
          label={t("apps.tree")}
          note={t("apps.treeNote")}
          text={t("apps.treeText")}
          value={apps.include_process_tree}
          onChange={set("include_process_tree")}
          disabled={!apps.enabled}
        />
        <NumberField
          label={t("apps.rescanEvery")}
          note={t("apps.rescanNote")}
          value={apps.rescan_ms}
          onChange={set("rescan_ms")}
          integer
          min={250}
          step={250}
          unit={t("units.ms")}
          disabled={!apps.enabled}
        />
        <FollowDeviceField
          label={t("apps.sampleRate")}
          note={t("apps.sampleRateNote")}
          value={apps.sample_rate}
          onChange={set("sample_rate")}
          fallback={48000}
          min={8000}
          step={100}
          unit={t("units.hz")}
          disabled={!apps.enabled}
        />
        <FollowDeviceField
          label={t("apps.channels")}
          note={t("apps.channelsNote")}
          value={apps.channels}
          onChange={set("channels")}
          fallback={2}
          min={1}
          max={8}
          disabled={!apps.enabled}
        />
      </Section>

      <Section title={t("apps.groupsTitle")} note={t("apps.groupsNote")}>
        {apps.groups.length === 0 && <p className="prose">{t("apps.noGroups")}</p>}

        {apps.groups.map((group, i) => (
          <GroupCard
            key={i}
            group={group}
            onChange={(next) => setGroups(replaceAt(apps.groups, i, next))}
            onRemove={() => setGroups(removeAt(apps.groups, i))}
          />
        ))}

        <div className="inline" style={{ marginTop: 10 }}>
          <button
            type="button"
            className="btn"
            onClick={() =>
              setGroups([...apps.groups, { name: t("apps.groupDefaultName", { n: apps.groups.length + 1 }), enabled: true, apps: [], gain: 1 }])
            }
          >
            {t("apps.addGroup")}
          </button>
        </div>
      </Section>

      <Section title={t("apps.runningTitle")} note={t("apps.runningNote")}>
        <div className="inline" style={{ marginBottom: 10 }}>
          <button type="button" className="btn" disabled={loading} onClick={() => void scan()}>
            {loading ? t("apps.scanning") : t("apps.scan")}
          </button>
        </div>
        {found && found.length === 0 && <p className="prose">{t("apps.nothingFound")}</p>}
        {found && found.length > 0 && (
          <table className="grid">
            <thead>
              <tr>
                <th>{t("apps.colApp")}</th>
                <th>{t("apps.colFile")}</th>
                <th className="num">{t("apps.colPid")}</th>
                <th>{t("apps.colState")}</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {found.map((app) => (
                <tr key={app.pid}>
                  <td>{app.display}</td>
                  <td>{app.exe}</td>
                  <td className="num">{app.pid}</td>
                  <td>{app.playing ? <span className="tag accent">{t("apps.playing")}</span> : <span className="tag">{t("apps.idle")}</span>}</td>
                  <td>
                    <button
                      type="button"
                      className="btn small"
                      disabled={apps.groups.length === 0}
                      onClick={() => {
                        const first = apps.groups[0];
                        if (!first || first.apps.includes(app.exe)) return;
                        setGroups(replaceAt(apps.groups, 0, { ...first, apps: [...first.apps, app.exe] }));
                      }}
                    >
                      {t("apps.take")}
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </Section>
    </>
  );
}

function GroupCard({ group, onChange, onRemove }: { group: AppGroup; onChange: (next: AppGroup) => void; onRemove: () => void }) {
  const { t } = useTranslation();
  const set = patcher(group, onChange);

  return (
    <div className="card">
      <header>
        <input type="checkbox" checked={group.enabled} onChange={(e) => set("enabled")(e.target.checked)} title={t("apps.groupActive")} />
        <div style={{ width: 220 }}>
          <input type="text" value={group.name} placeholder={t("apps.groupName")} onChange={(e) => set("name")(e.target.value)} />
        </div>
        <span className="spacer" />
        <button type="button" className="btn small danger" onClick={onRemove}>
          {t("apps.remove")}
        </button>
      </header>
      <div className="body">
        <Field label={t("apps.programs")} note={t("apps.programsNote")}>
          <textarea
            style={{ minHeight: 78 }}
            value={group.apps.join("\n")}
            spellCheck={false}
            onChange={(e) =>
              set("apps")(e.target.value.split("\n").map((line) => line.trim()).filter((line, i, all) => line !== "" || i === all.length - 1),)
            }
          />
        </Field>
        <NumberField label={t("apps.gain")} note={t("apps.gainNote")} value={group.gain} onChange={set("gain")} min={0} step={0.1} />
      </div>
    </div>
  );
}

/**
 * A capture format value that is normally not stated at all.
 *
 * Per-app capture has no mix format of its own, so by default it takes the output device's — that is what makes a 7.1 setup arrive as 7.1 instead of
 * as the stereo a fixed number would pin it to. Stating one is still possible, which is what the checkbox is for.
 */
function FollowDeviceField({
  label,
  note,
  value,
  onChange,
  fallback,
  min,
  max,
  step,
  unit,
  disabled,
}: {
  label: string;
  note?: string;
  value: number | null;
  onChange: (v: number | null) => void;
  fallback: number;
  min?: number;
  max?: number;
  step?: number;
  unit?: string;
  disabled?: boolean;
}) {
  const { t } = useTranslation();

  return (
    <Field label={label} note={note}>
      <div className="inline">
        <label className="inline" style={{ cursor: disabled ? "default" : "pointer" }}>
          <input type="checkbox" checked={value === null} disabled={disabled} onChange={(e) => onChange(e.target.checked ? null : fallback)} />
          <span className="prose">{t("apps.followDevice")}</span>
        </label>
        {value !== null && (
          <>
            <div style={{ width: 130 }}>
              <NumberInput value={value} onChange={onChange} integer min={min} max={max} step={step} disabled={disabled} />
            </div>
            {unit && <span className="prose">{unit}</span>}
          </>
        )}
      </div>
    </Field>
  );
}
