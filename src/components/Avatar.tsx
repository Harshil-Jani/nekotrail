import { useState } from "react";

/** A round avatar that falls back to the first letter when the image can't load (offline, renamed user). */
export function Avatar({ src, size, label, title }: { src: string; size: number; label: string; title?: string }) {
  const [failed, setFailed] = useState(false);
  if (failed || !src) {
    return (
      <span className="avatar avatar-fallback" style={{ width: size, height: size, fontSize: size * 0.45 }} title={title} aria-label={label}>
        {label.replace(/^@/, "").charAt(0).toUpperCase()}
      </span>
    );
  }
  return <img className="avatar" src={src} width={size} height={size} alt={label} title={title} onError={() => setFailed(true)} />;
}
