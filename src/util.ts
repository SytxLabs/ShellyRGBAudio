/** `const set = patcher(section, onChange)` then `onChange={set("fft_size")}` — shallow immutable field updates without repeating the spread. */
export function patcher<T extends object>(value: T, onChange: (next: T) => void) {
  return <K extends keyof T>(key: K) =>
    (next: T[K]) =>
      onChange({ ...value, [key]: next });
}

/** Replaces one element of an array without mutating it. */
export function replaceAt<T>(list: readonly T[], index: number, item: T): T[] {
  return list.map((existing, i) => (i === index ? item : existing));
}

export function removeAt<T>(list: readonly T[], index: number): T[] {
  return list.filter((_, i) => i !== index);
}

export function moveItem<T>(list: readonly T[], from: number, to: number): T[] {
  if (to < 0 || to >= list.length) return [...list];
  const next = [...list];
  const [item] = next.splice(from, 1);
  if (item !== undefined) next.splice(to, 0, item);
  return next;
}

export function formatHz(hz: number): string {
  return hz >= 1000 ? `${(hz / 1000).toFixed(hz % 1000 === 0 ? 0 : 1)} kHz` : `${Math.round(hz)} Hz`;
}

export function formatSampleRate(hz: number): string {
  const khz = hz / 1000;
  return Math.abs(khz - Math.round(khz)) < 0.05 ? `${khz.toFixed(0)} kHz` : `${khz.toFixed(1)} kHz`;
}
