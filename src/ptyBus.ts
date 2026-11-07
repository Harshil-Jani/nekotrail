import { listen } from "@tauri-apps/api/event";
import { makePtyBus } from "./pty";

/** The window's one terminal output router (see `makePtyBus`). */
export const ptyBus = makePtyBus(listen);
