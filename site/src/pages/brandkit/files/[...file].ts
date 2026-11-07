// Serves each file of the repo's brand/ folder at /brandkit/files/<path>, so the page links the
// exact files scripts/brand.ts wrote, with no copy kept in public/.
import type { APIRoute } from "astro";
import { kitFiles, kitFile } from "../../../lib/brand";

export function getStaticPaths() {
  return kitFiles().map((file) => ({ params: { file } }));
}

const TYPES: Record<string, string> = { svg: "image/svg+xml", png: "image/png", json: "application/json", md: "text/markdown" };

export const GET: APIRoute = ({ params }) => {
  const file = String(params.file);
  const ext = file.split(".").pop() ?? "";
  return new Response(new Uint8Array(kitFile(file)), { headers: { "Content-Type": TYPES[ext] ?? "application/octet-stream" } });
};
