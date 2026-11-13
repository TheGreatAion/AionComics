import { listen as tauriListen, type EventCallback, type UnlistenFn } from "@tauri-apps/api/event";
import { remote } from "./api";

/** News from AionComics (a scan finished, details arrived). Only the window hears it: on another
 *  device, screens refresh when you come back to the page instead. */
export function listen<T>(event: string, handler: EventCallback<T>): Promise<UnlistenFn> {
  return remote ? Promise.resolve(() => {}) : tauriListen<T>(event, handler);
}
