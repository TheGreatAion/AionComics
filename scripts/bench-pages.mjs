// Page-jump benchmark: how long pages take to show in the reader when jumping and skipping.
//
// Open a book in a copy of AionComics with its webview open to DevTools, then run this script:
//   set WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9333
//   AionComics.exe "H:\Comics\...\Book.cbz"
//   node scripts/bench-pages.mjs
//
// Random jumps through the page box (time until the page is on screen), then fast skips of 15
// pages with Page Down (time from the last key until the page you stopped on shows). The
// reader marks both moments (see PageView.svelte). It reads the book, so the reading position
// and sittings change: put them back afterward.

const targets = await (await fetch("http://127.0.0.1:9333/json")).json();
const ws = new WebSocket(targets.find((t) => t.type === "page").webSocketDebuggerUrl);
await new Promise((r) => (ws.onopen = r));
let seq = 0;
const send = (method, params = {}) =>
  new Promise((res) => {
    const i = ++seq;
    const h = (m) => {
      const d = JSON.parse(m.data);
      if (d.id === i) (ws.removeEventListener("message", h), res(d.result));
    };
    ws.addEventListener("message", h);
    ws.send(JSON.stringify({ id: i, method, params }));
  });
const ev = async (x) => (await send("Runtime.evaluate", { expression: x, awaitPromise: true, returnByValue: true })).result?.value;
const wait = (ms) => new Promise((r) => setTimeout(r, ms));
const CODES = { PageDown: 34, Enter: 13, g: 71 };
const key = async (k) => {
  const code = k.length === 1 ? `Key${k.toUpperCase()}` : k;
  for (const type of ["rawKeyDown", "keyUp"]) await send("Input.dispatchKeyEvent", { type, key: k, code, windowsVirtualKeyCode: CODES[k] });
};

// Start the clock in the page, just before an action.
const start = () => ev(`window.__benchT = performance.now()`);
// Milliseconds from the clock's start until the page the reader settled on is on screen (null
// after 5 s), and until something of it was (its preview, if that came first). The page asked
// for last after the start is the one that counts.
const settled = () =>
  ev(`new Promise((res) => {
    const t0 = window.__benchT;
    const marks = (name) => performance.getEntriesByName(name).filter((m) => m.startTime >= t0);
    const look = () => {
      const want = marks("aion:page").at(-1);
      const at = (name) => want && marks(name).find((m) => m.detail === want.detail && m.startTime >= want.startTime);
      const shown = at("aion:shown");
      if (shown) {
        const seen = Math.min(shown.startTime, at("aion:preview")?.startTime ?? Infinity);
        return res([Math.round(shown.startTime - t0), Math.round(seen - t0)]);
      }
      if (performance.now() - t0 > 5000) return res([null, null]);
      setTimeout(look, 2);
    };
    setTimeout(look, 30);
  })`);
const median = (xs) => {
  const v = xs.filter((x) => x !== null).sort((a, b) => a - b);
  return v.length ? v[Math.floor(v.length / 2)] : null;
};

// The reader is up once a page image is; in Guided View, switch to the normal page view (V).
const CODES_V = 86;
for (let k = 0; !(await ev(`!!document.querySelector("img[data-page]")`)); k++) {
  if (k === 25 && (await ev(`!!document.querySelector(".reader img")`))) {
    for (const type of ["rawKeyDown", "keyUp"]) await send("Input.dispatchKeyEvent", { type, key: "v", code: "KeyV", windowsVirtualKeyCode: CODES_V });
  }
  await wait(200);
}
await wait(1500);
// The page count, from the "/ 192" beside the page box.
const n = await ev(`+([...document.querySelectorAll(".dim")].map((e) => e.textContent.trim().match(/^\\/\\s*(\\d+)$/)).find(Boolean)?.[1] ?? 0)`);
// Random jumps, spread over the book, in an order that keeps moving far.
const jumps = [0.31, 0.83, 0.12, 0.57, 0.94, 0.45, 0.68, 0.21, 0.76, 0.38].map((f) => Math.max(1, Math.floor(f * n)));
const times = [];
const seens = [];
for (const p of jumps) {
  await key("g");
  await send("Input.insertText", { text: String(p + 1) });
  await start();
  await key("Enter");
  const [full, seen] = await settled();
  times.push(full);
  seens.push(seen);
  await wait(400);
}
// Skips of 15 pages with Page Down (forward in manga too), a key every 35 ms (a held key repeats
// about that fast),
// from three places in the book.
const skips = [];
const skipSeens = [];
for (const f of [0.15, 0.5, 0.8]) {
  await key("g");
  await send("Input.insertText", { text: String(Math.max(1, Math.min(Math.floor(f * n), n - 36)) + 1) });
  await key("Enter");
  await wait(800);
  // 15 presses, fewer in a short book (two pages a press in double page mode).
  const presses = Math.max(3, Math.min(15, Math.floor((n - 4) / 4)));
  for (let k = 0; k < presses - 1; k++) (await key("PageDown"), await wait(35));
  await start();
  await key("PageDown");
  const [full, seen] = await settled();
  skips.push(full);
  skipSeens.push(seen);
  await wait(400);
}
const worst = (xs) => (xs.includes(null) ? null : Math.max(...xs));
console.log(JSON.stringify({ pages: n, jump: median(times), jumpWorst: worst(times), skip: median(skips), skipWorst: worst(skips), seen: median(seens), seenWorst: worst(seens), skipSeen: median(skipSeens), jumps: times, skips }));
ws.close();
