// The whole brand kit as one zip, built from brand/ at site build time.
import type { APIRoute } from "astro";
import { kitFiles, kitFile } from "../../lib/brand";
import { zip } from "../../lib/zip";

export const GET: APIRoute = () => {
  const files = kitFiles().map((f) => ({ name: `nekotrail-brand-kit/${f}`, data: new Uint8Array(kitFile(f)) }));
  return new Response(zip(files), { headers: { "Content-Type": "application/zip" } });
};
