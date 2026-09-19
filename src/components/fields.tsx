import { useEffect, useId, useState, type ReactNode } from "react";

export function Section({ title, note, children }: { title: string; note?: string; children: ReactNode }) {
  return (
    <section className="section">
      <header>
        <h2>{title}</h2>
        {note && <p className="prose">{note}</p>}
      </header>
      <div className="body">{children}</div>
    </section>
  );
}

export function Field({ label, note, children, wide }: { label?: string; note?: string; children: ReactNode; wide?: boolean }) {
  const id = useId();
  return (
    <div className={wide ? "field wide" : "field"}>
      {label !== undefined && <label htmlFor={id}>{label}</label>}
      <div className="control">
        {/* The id lands on the first form control the caller renders; good enough for the label association React needs. */}
        <div id={id}>{children}</div>
        {note && <p className="note prose">{note}</p>}
      </div>
    </div>
  );
}

/**
 * Numbers need a text buffer of their own: binding `value` straight to the number means clearing the box to retype it immediately snaps back to the old
 * value, and a half-typed "-" or "0." is not parseable yet either.
 */
export function NumberInput({
  value,
  onChange,
  min,
  max,
  step,
  integer,
  disabled,
}: {
  value: number;
  onChange: (v: number) => void;
  min?: number;
  max?: number;
  step?: number;
  integer?: boolean;
  disabled?: boolean;
}) {
  const [text, setText] = useState(String(value));

  useEffect(() => {
    // Only follow the prop when it really differs, or every keystroke would be rewritten from the parsed value.
    if (Number.parseFloat(text) !== value) setText(String(value));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [value]);

  return (
    <input
      type="number"
      value={text}
      min={min}
      max={max}
      step={step ?? (integer ? 1 : "any")}
      disabled={disabled}
      onChange={(e) => {
        setText(e.target.value);
        const parsed = integer ? Number.parseInt(e.target.value, 10) : Number.parseFloat(e.target.value);
        if (Number.isFinite(parsed)) onChange(parsed);
      }}
      onBlur={() => setText(String(value))}
    />
  );
}

export function NumberField(props: {
  label: string;
  note?: string;
  value: number;
  onChange: (v: number) => void;
  min?: number;
  max?: number;
  step?: number;
  integer?: boolean;
  unit?: string;
  disabled?: boolean;
}) {
  const { label, note, unit, ...rest } = props;
  return (
    <Field label={label} note={note}>
      <div className="inline">
        <div style={{ width: 130 }}>
          <NumberInput {...rest} />
        </div>
        {unit && <span className="prose">{unit}</span>}
      </div>
    </Field>
  );
}

export function SliderField({
  label,
  note,
  value,
  onChange,
  min,
  max,
  step = 0.01,
  disabled,
}: {
  label: string;
  note?: string;
  value: number;
  onChange: (v: number) => void;
  min: number;
  max: number;
  step?: number;
  disabled?: boolean;
}) {
  return (
    <Field label={label} note={note}>
      <div className="slider">
        <input type="range" min={min} max={max} step={step} value={value} disabled={disabled} onChange={(e) => onChange(Number(e.target.value))} />
        <NumberInput value={value} onChange={onChange} min={min} max={max} step={step} disabled={disabled} />
      </div>
    </Field>
  );
}

export function TextField({
  label,
  note,
  value,
  onChange,
  placeholder,
  password,
  disabled,
}: {
  label: string;
  note?: string;
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
  password?: boolean;
  disabled?: boolean;
}) {
  return (
    <Field label={label} note={note}>
      {/* Masked rather than omitted: the value has to stay editable, and the file it is written to is plain text either way. */}
      <input
        type={password ? "password" : "text"}
        value={value}
        placeholder={placeholder}
        disabled={disabled}
        onChange={(e) => onChange(e.target.value)}
      />
    </Field>
  );
}

export function SelectField<T extends string>({
  label,
  note,
  value,
  onChange,
  options,
  disabled,
}: {
  label: string;
  note?: string;
  value: T;
  onChange: (v: T) => void;
  options: readonly (readonly [T, string])[];
  disabled?: boolean;
}) {
  return (
    <Field label={label} note={note}>
      <select value={value} disabled={disabled} onChange={(e) => onChange(e.target.value as T)}>
        {options.map(([v, text]) => (
          <option key={v} value={v}>
            {text}
          </option>
        ))}
      </select>
    </Field>
  );
}

export function CheckboxField({
  label,
  note,
  text,
  value,
  onChange,
  disabled,
}: {
  label: string;
  note?: string;
  text?: string;
  value: boolean;
  onChange: (v: boolean) => void;
  disabled?: boolean;
}) {
  return (
    <Field label={label} note={note}>
      <label className="inline" style={{ cursor: disabled ? "default" : "pointer" }}>
        <input type="checkbox" checked={value} disabled={disabled} onChange={(e) => onChange(e.target.checked)} />
        {text && <span>{text}</span>}
      </label>
    </Field>
  );
}

/**
 * Colours are hex strings on the wire, because that is what `Rgbw` serializes to — `#RRGGBB`, or `#RRGGBBWW` when a device has a white channel.
 * `<input type="color">` only understands the six-digit form, so the picker edits the RGB half and the text box keeps the whole value reachable.
 */
export function ColorPicker({ value, onChange, disabled }: { value: string; onChange: (v: string) => void; disabled?: boolean }) {
  const rgb = toSixDigit(value);
  const white = value.replace("#", "").length === 8 ? value.slice(-2) : "";

  return (
    <div className="inline">
      <input type="color" value={rgb} disabled={disabled} onChange={(e) => onChange(`${e.target.value}${white}`.toUpperCase())} />
      <div style={{ width: 118 }}>
        <input type="text" value={value} disabled={disabled} spellCheck={false} onChange={(e) => onChange(e.target.value)} />
      </div>
    </div>
  );
}

export function ColorField({
  label,
  note,
  value,
  onChange,
  nullLabel,
}: {
  label: string;
  note?: string;
  /** `null` where the config allows "no colour set". */
  value: string | null;
  onChange: (v: string | null) => void;
  nullLabel?: string;
}) {
  return (
    <Field label={label} note={note}>
      <div className="inline">
        {nullLabel !== undefined && (
          <label className="inline" style={{ cursor: "pointer" }}>
            <input type="checkbox" checked={value !== null} onChange={(e) => onChange(e.target.checked ? "#FF0000" : null)} />
            <span className="prose">{nullLabel}</span>
          </label>
        )}
        {value !== null && <ColorPicker value={value} onChange={onChange} />}
      </div>
    </Field>
  );
}

/** Accepts `#RGB`, `#RGBA`, `#RRGGBB` and `#RRGGBBWW` the way the Rust side does, and returns something `<input type="color">` will take. */
export function toSixDigit(hex: string): string {
  const digits = hex.replace("#", "");
  const expand = (s: string) =>
    s
      .split("")
      .map((c) => c + c)
      .join("");

  if (digits.length === 3 || digits.length === 4) return `#${expand(digits.slice(0, 3))}`;
  if (digits.length >= 6) return `#${digits.slice(0, 6)}`;
  return "#000000";
}
