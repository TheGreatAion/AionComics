// New versions of AionComics: looked for on GitHub, downloaded in the background, installed on a
// restart when you say so. What changed comes with them (the release notes), and the first start
// after updating shows it once.

import { getVersion } from "@tauri-apps/api/app";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { api, remote } from "./api";

type Phase = "idle" | "checking" | "none" | "found" | "downloading" | "ready" | "installing" | "error";

export const updates = $state({
  /** This copy's version, "1.3.0". */
  current: "",
  /** The newer version out, and what changed in it. */
  next: null as null | { version: string; date: string | null; notes: string },
  phase: "idle" as Phase,
  /** While downloading, 0 to 1. */
  progress: 0,
  error: "",
  /** The portable exe can't replace itself; it sends you to the download instead. */
  portable: false,
  /** Download new versions without being asked (Settings). */
  auto: true,
  /** The What's new sheet: about a version waiting to install, or the one just installed. */
  sheet: null as null | "next" | "after",
});

const AUTO = "aion.autoUpdate";
const SEEN = "aion.seenVersion";
let pending: Update | null = null;

const stored = (k: string) => {
  try {
    return localStorage.getItem(k);
  } catch {
    return null;
  }
};
const store = (k: string, v: string) => {
  try {
    localStorage.setItem(k, v);
  } catch {
    // Asked again next time.
  }
};

/** The release notes bundled with this copy, by version. */
const NOTES = import.meta.glob("/release-notes/*.md", { query: "?raw", import: "default", eager: true }) as Record<string, string>;
export const notesFor = (version: string) => NOTES[`/release-notes/v${version}.md`] ?? "";

/** Once, when the app starts. */
export async function startUpdates() {
  if (remote) return;
  updates.current = await getVersion();
  updates.auto = stored(AUTO) !== "0";
  updates.portable = !(await api.installed().catch(() => true));
  // The first start after an update: what changed.
  const seen = stored(SEEN);
  if (seen && seen !== updates.current && notesFor(updates.current)) updates.sheet = "after";
  store(SEEN, updates.current);
  if (updates.auto) {
    // A little after starting, then twice a day while it's open.
    setTimeout(() => look(true), 10_000);
    setInterval(() => look(true), 12 * 3600_000);
  }
}

export function setAuto(on: boolean) {
  updates.auto = on;
  store(AUTO, on ? "1" : "0");
  if (on && updates.phase === "idle") look(true);
}

/** Look for a newer version. In the background one is downloaded at once (not for the portable copy). */
export async function look(background = false) {
  if (remote || ["checking", "downloading", "installing"].includes(updates.phase)) return;
  if (updates.phase === "ready") return;
  updates.phase = "checking";
  updates.error = "";
  try {
    const u = await check();
    if (!u) {
      updates.phase = "none";
      return;
    }
    pending = u;
    updates.next = { version: u.version, date: u.date ?? null, notes: u.body ?? "" };
    updates.phase = "found";
    if (background && !updates.portable) await download();
  } catch (e) {
    updates.phase = background ? "idle" : "error";
    updates.error = String(e);
  }
}

export async function download() {
  if (!pending || updates.portable) return;
  updates.phase = "downloading";
  updates.progress = 0;
  let total = 0;
  let got = 0;
  try {
    await pending.download((ev) => {
      if (ev.event === "Started") total = ev.data.contentLength ?? 0;
      else if (ev.event === "Progress") {
        got += ev.data.chunkLength;
        updates.progress = total ? got / total : 0;
      }
    });
    updates.phase = "ready";
  } catch (e) {
    updates.phase = "error";
    updates.error = String(e);
  }
}

/** Install what's downloaded (downloading it first if need be) and start again on the new version. */
export async function install() {
  if (updates.portable) {
    if (updates.next) await api.openRelease(updates.next.version);
    return;
  }
  if (!pending) return;
  if (updates.phase !== "ready") await download();
  if (updates.phase !== "ready") return;
  updates.phase = "installing";
  try {
    await pending.install();
    await relaunch();
  } catch (e) {
    updates.phase = "error";
    updates.error = String(e);
  }
}

/** "1.3.0" as people say it: "1.3". */
export const shortVersion = (v: string) => v.replace(/\.0$/, "");

/** Release notes as the sheet shows them: the "### Area" sections and their bullets, without the
 * download list at the top or the line about Windows at the end. */
export function noteSections(md: string): { title: string; items: { lead: string; rest: string }[] }[] {
  const out: { title: string; items: { lead: string; rest: string }[] }[] = [];
  for (const raw of md.split(/\r?\n/)) {
    const line = raw.trim();
    const h = /^###\s+(.+)$/.exec(line);
    if (h) {
      out.push({ title: h[1], items: [] });
      continue;
    }
    const b = /^[-*]\s+(.+)$/.exec(line);
    if (!b || !out.length) continue;
    const m = /^\*\*(.+?)\*\*\s*(.*)$/.exec(b[1]);
    out[out.length - 1].items.push(m ? { lead: m[1], rest: m[2] } : { lead: "", rest: b[1] });
  }
  return out.filter((s) => s.items.length);
}
