// A custom accent must stay visible on the theme's background and keep its button text readable.

type Rgb = [number, number, number];

const parse = (hex: string): Rgb => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16)) as Rgb;
const toHex = (rgb: Rgb) => "#" + rgb.map((c) => Math.round(c).toString(16).padStart(2, "0")).join("");

/** WCAG relative luminance */
function luminance(rgb: Rgb) {
  const [r, g, b] = rgb.map((c) => {
    const s = c / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

const contrast = (a: number, b: number) => (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);

// Luminance of --bg in app.css
const BG_DARK = luminance([27, 27, 31]);
const BG_LIGHT = luminance([243, 243, 246]);

/** Moves the color toward white (dark theme) or black (light theme) until it stands out from the background. */
export function fitAccent(hex: string, dark: boolean): { accent: string; text: string } {
  let rgb = parse(hex);
  const bg = dark ? BG_DARK : BG_LIGHT;
  const target: Rgb = dark ? [255, 255, 255] : [0, 0, 0];
  for (let i = 0; i < 20 && contrast(luminance(rgb), bg) < 3; i++)
    rgb = rgb.map((c, k) => c + (target[k] - c) * 0.1) as Rgb;
  const l = luminance(rgb);
  // White text like the presets, dark only when white would be hard to read
  return { accent: toHex(rgb), text: contrast(l, 1) >= 3 ? "#ffffff" : "#1b1b1f" };
}
