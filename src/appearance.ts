/**
 * Appearance: app zoom, light or dark, and the code and terminal fonts.
 * Everything here is cheap at startup: a few CSS variables and one zoom call. Font detection
 * only runs when the Appearance pane opens.
 */
import { useEffect, useState } from "react";

export type Theme = "system" | "light" | "dark";

export const ZOOM_MIN = 0.8;
export const ZOOM_MAX = 1.5;
export const ZOOM_STEP = 0.1;
/** The choices in the Appearance pane, 80% to 150%. */
export const ZOOM_LEVELS = [0.8, 0.9, 1, 1.1, 1.2, 1.3, 1.4, 1.5];

/** The default code and terminal look, same as before Appearance existed. */
export const CODE_SIZE = 12;
export const TERM_SIZE = 13;
export const CODE_STACK = 'ui-monospace, "SF Mono", SFMono-Regular, Menlo, monospace';
export const TERM_STACK = "ui-monospace, SFMono-Regular, Menlo, monospace";

/** Monospace fonts people commonly have on a Mac. Only the installed ones are offered. */
export const MONO_FONTS = [
  "SF Mono", "Menlo", "Monaco", "JetBrains Mono", "Fira Code", "Cascadia Code", "Source Code Pro",
  "IBM Plex Mono", "Hack", "Iosevka", "Berkeley Mono", "Roboto Mono", "Ubuntu Mono", "Courier New",
];

export const clampZoom = (z: number) => Math.round(Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, Number.isFinite(z) ? z : 1)) * 100) / 100;

/** ⌘+ and ⌘- move by 10%, landing on the 80% to 150% steps. */
export const stepZoom = (z: number, dir: 1 | -1) => clampZoom(Math.round((z + dir * ZOOM_STEP) * 10) / 10);

/** A font choice as a CSS stack. "" means the default; a named font falls back to the default. */
export function fontStack(name: string, fallback: string) {
  const n = name.trim().replace(/["\\]/g, "");
  return n ? `"${n}", ${fallback}` : fallback;
}

/** Whether the app should be dark, given the setting and what the system prefers. */
export const resolveDark = (theme: Theme, systemDark: boolean) => (theme === "system" ? systemDark : theme === "dark");

/**
 * Snapshot pixels per CSS pixel, as getBoundingClientRect measures them. The snapshot is the
 * whole window at screen pixels. With webview zoom, the root box is already the zoomed viewport;
 * with CSS zoom (the browser preview) it is not, so we measure the root instead of assuming 1.
 */
export const snapshotScale = (bitmapWidth: number, rootWidth: number) => (rootWidth > 0 ? bitmapWidth / rootWidth : 1);

// ---------- zoom ----------

const ZOOM_KEY = "nekotrail.zoom";
/** The real app zooms the web view itself (crisp text, real window size). The preview uses CSS zoom. */
let webviewZoom: ((z: number) => Promise<void>) | null = null;
export function setZoomBackend(fn: (z: number) => Promise<void>) { webviewZoom = fn; }

export const getZoom = () => clampZoom(Number(localStorage.getItem(ZOOM_KEY)) || 1);

/** The browser preview's fallback. CSS zoom also scales vh units, so the app frame uses 100% heights. */
function cssZoom(z: number) {
  const root = document.documentElement.style;
  if (z === 1) root.removeProperty("zoom");
  else root.zoom = String(z);
}

export function applyZoom(z = getZoom()) {
  if (webviewZoom) {
    cssZoom(1);
    webviewZoom(z).catch(() => cssZoom(z));
  } else {
    cssZoom(z);
  }
}

export function setZoom(z: number) {
  const next = clampZoom(z);
  localStorage.setItem(ZOOM_KEY, String(next));
  applyZoom(next);
  window.dispatchEvent(new CustomEvent("nekotrail-zoom", { detail: next }));
}

/** The zoom level, kept current when ⌘+ / ⌘- / ⌘0 change it elsewhere. */
export function useZoom() {
  const [z, setZ] = useState(getZoom);
  useEffect(() => {
    const on = (e: Event) => setZ((e as CustomEvent<number>).detail);
    window.addEventListener("nekotrail-zoom", on);
    return () => window.removeEventListener("nekotrail-zoom", on);
  }, []);
  return z;
}

// ---------- theme ----------

const darkQuery = () => (typeof matchMedia === "function" ? matchMedia("(prefers-color-scheme: dark)") : null);
let currentTheme: Theme = "system";
let setWindowTheme: ((t: "light" | "dark" | null) => Promise<void>) | null = null;
/** In the app, the window's own appearance follows too, so the sidebar vibrancy matches. */
export function setThemeBackend(fn: (t: "light" | "dark" | null) => Promise<void>) { setWindowTheme = fn; }

export function applyTheme(theme: Theme) {
  if (theme === currentTheme && document.documentElement.dataset.theme === theme) return;
  currentTheme = theme;
  document.documentElement.dataset.theme = theme;
  if (theme === "system") document.documentElement.style.removeProperty("color-scheme");
  else document.documentElement.style.colorScheme = theme;
  setWindowTheme?.(theme === "system" ? null : theme).catch(() => {});
  window.dispatchEvent(new Event("nekotrail-theme"));
}

export const isDark = () => resolveDark(currentTheme, darkQuery()?.matches ?? false);

/** True while the app shows dark, following both the setting and the system. */
export function useDark() {
  const [dark, setDark] = useState(isDark);
  useEffect(() => {
    const on = () => setDark(isDark());
    const q = darkQuery();
    q?.addEventListener("change", on);
    window.addEventListener("nekotrail-theme", on);
    return () => { q?.removeEventListener("change", on); window.removeEventListener("nekotrail-theme", on); };
  }, []);
  return dark;
}

// ---------- fonts ----------

/** Code font and size go into CSS variables used by diffs, chat code blocks and tool output. */
export function applyCodeFont(font: string, size: number) {
  const root = document.documentElement.style;
  if (font) root.setProperty("--mono", fontStack(font, CODE_STACK));
  else root.removeProperty("--mono");
  if (size && size !== CODE_SIZE) root.setProperty("--code-size", `${size}px`);
  else root.removeProperty("--code-size");
}

/**
 * Which of the given fonts are installed. A canvas width test: text drawn in the font is compared
 * with the same text in two generic fallbacks. document.fonts.check() is not used because it says
 * yes to any font that has nothing to download, installed or not.
 */
export function installedFonts(names: string[]): string[] {
  const g = document.createElement("canvas").getContext("2d");
  if (!g) return [];
  const sample = "mmmmmmmmmmlli10O@#{}";
  const width = (font: string) => { g.font = `72px ${font}`; return g.measureText(sample).width; };
  const bases = ["monospace", "sans-serif", "serif"].map((b) => [b, width(b)] as const);
  return names.filter((n) => bases.some(([b, w]) => Math.abs(width(`"${n}", ${b}`) - w) > 0.5));
}

/** Installed monospace fonts besides SF Mono, which is the default (the system's ui-monospace). */
export function availableMonoFonts(): string[] {
  return installedFonts(MONO_FONTS.filter((n) => n !== "SF Mono"));
}

// ---------- all together ----------

/** Apply the saved appearance now and again whenever settings change. Call once at startup. */
export function startAppearance(read: () => { theme: Theme; codeFont: string; codeSize: number }) {
  const apply = () => {
    const s = read();
    applyTheme(s.theme);
    applyCodeFont(s.codeFont, s.codeSize);
  };
  apply();
  applyZoom();
  window.addEventListener("nekotrail-settings", apply);
  return () => window.removeEventListener("nekotrail-settings", apply);
}
