import { useEffect, useState } from "react";

/**
 * Reads CSS custom properties and follows the theme.
 *
 * The room view draws with three.js, which knows nothing about CSS, so the palette has to be handed to it as plain strings — and re-handed whenever the
 * viewer switches theme or Windows does it for them.
 */
export function useThemeColors<K extends string>(names: Record<K, string>): Record<K, string> {
  const read = () => {
    const style = getComputedStyle(document.documentElement);
    const out = {} as Record<K, string>;
    for (const key of Object.keys(names) as K[]) {
      out[key] = style.getPropertyValue(names[key]).trim() || "#888888";
    }
    return out;
  };

  const [colors, setColors] = useState(read);

  useEffect(() => {
    const update = () => setColors(read());

    // The in-app switch stamps data-theme on <html>; "system" leaves it off and lets the media query decide.
    const observer = new MutationObserver(update);
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });

    const media = window.matchMedia("(prefers-color-scheme: dark)");
    media.addEventListener("change", update);

    return () => {
      observer.disconnect();
      media.removeEventListener("change", update);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return colors;
}
