import { open } from "@tauri-apps/plugin-dialog";
import { showMenu } from "./state.svelte";

/** What a + button adds to: a series, a new series in a collection, or with neither, a first series. */
export type AddTarget = { series?: number; collection?: number; title: string };

/** Comics chosen with a + button, waiting to be confirmed (see DropZone). */
export const adding = $state({
  ask: null as { paths: string[]; target: AddTarget } | null,
  /** The new library box is open (see DropZone). */
  newLibrary: false,
});

async function choose(target: AddTarget, folder: boolean) {
  const picked = folder
    ? await open({ directory: true, title: `Add to ${target.title}` })
    : await open({
        multiple: true,
        title: `Add to ${target.title}`,
        filters: [{ name: "Comics", extensions: ["cbz", "cbr", "zip", "rar"] }],
      });
  const paths = [picked ?? []].flat();
  if (paths.length) adding.ask = { paths, target };
}

/** The + buttons' menu: comic files, or a folder of them. */
export function addMenu(e: MouseEvent, target: AddTarget) {
  showMenu(e, [
    { label: "Choose comics…", action: () => choose(target, false) },
    { label: "Choose a folder…", action: () => choose(target, true) },
  ]);
}
