import { useEffect, useState } from "react";
import { api } from "./api";

// One lookup per repo per launch: the sidebar row and every picker share it.
const cache = new Map<string, Promise<string>>();

/** The repo's picture: its own cached logo, else the GitHub owner's avatar, else "" (Avatar draws a letter). */
export function repoAvatarUrl(id: string): Promise<string> {
  let p = cache.get(id);
  if (!p) {
    p = api.repoAvatar(id).then((u) => u || Promise.reject())
      .catch(() => api.repoGithub(id).then((g) => g?.owner_avatar ?? ""))
      .catch(() => "");
    cache.set(id, p);
  }
  return p;
}

export function useRepoAvatar(id: string) {
  const [src, setSrc] = useState("");
  useEffect(() => {
    let live = true;
    setSrc("");
    repoAvatarUrl(id).then((u) => live && setSrc(u));
    return () => { live = false; };
  }, [id]);
  return src;
}
