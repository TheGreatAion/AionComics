<script lang="ts" module>
  // Solid icons on a 24px grid, drawn for AionComics so the reader's toolbar reads as one family:
  // each closed shape (a page, the bookmark, the play button) is filled, everything else is a
  // heavy line.
  const PATHS = {
    back: "M19 12H5M11 18l-6-6 6-6",
    strip: "M3 6h4v12H3zM10 6h4v12h-4zM17 6h4v12h-4z",
    goto: "M4 9h16M4 15h16M10 3 8 21M16 3l-2 18",
    fitHeight: "M12 3v18M9 6l3-3 3 3M9 18l3 3 3-3",
    fitWidth: "M3 12h18M6 9l-3 3 3 3M18 9l3 3-3 3",
    fitPage: "M7 3h10v18H7z",
    scroll: "M6 2h12v8H6zM6 14h12v8H6z",
    double: "M3 5h7.5v14H3zM13.5 5H21v14h-7.5z",
    doubleRtl: "M3 5h7.5v14H3zM13.5 5H21v14h-7.5zM8 10l-2 2 2 2",
    rotateLeft: "M3 12a9 9 0 1 0 2.6-6.4L3 8M3 3v5h5",
    rotateRight: "M21 12a9 9 0 1 1-2.6-6.4L21 8M21 3v5h-5",
    zoomIn: "M10.5 17.5a7 7 0 1 0 0-14 7 7 0 0 0 0 14zM20 20l-4.5-4.5M7.5 10.5h6M10.5 7.5v6",
    zoomOut: "M10.5 17.5a7 7 0 1 0 0-14 7 7 0 0 0 0 14zM20 20l-4.5-4.5M7.5 10.5h6",
    magnifier: "M10.5 17.5a7 7 0 1 0 0-14 7 7 0 0 0 0 14zM20 20l-4.5-4.5M8 10.5a2.5 2.5 0 0 1 2.5-2.5",
    bookmark: "M6 3h12v18l-6-4.5L6 21z",
    bookmarkOutline: "M6 3h12v18l-6-4.5L6 21z",
    bookmarks: "M5 3h10v16l-5-3.8L5 19zM19 6v15",
    guided: "M3 8V3h5M16 3h5v5M21 16v5h-5M8 21H3v-5M9 9h6v6H9z",
    editPanels: "M3 3h18v18H3zM3 11h18M12 11v10",
    more: "M5 12h.01M12 12h.01M19 12h.01",
    fullscreen: "M8 3H3v5M16 3h5v5M21 16v5h-5M8 21H3v-5",
    exitFullscreen: "M3 8h5V3M21 8h-5V3M16 21v-5h5M3 16h5v5",
    minimize: "M5 12h14",
    maximize: "M5.5 5.5h13v13h-13z",
    restore: "M8.5 8.5h10v10h-10zM5.5 15.5v-10h10",
    close: "M6 6l12 12M18 6 6 18",
    options: "M4 7h10M18 7h2M4 17h4M12 17h8M16 5v4M10 15v4",
    search: "M10.5 17.5a7 7 0 1 0 0-14 7 7 0 0 0 0 14zM20 20l-4.5-4.5",
    rescan: "M20 12a8 8 0 1 1-2.34-5.66M20 4v5h-5",
    folder: "M3 6.5A1.5 1.5 0 0 1 4.5 5H9l2 2h8.5A1.5 1.5 0 0 1 21 8.5v9a1.5 1.5 0 0 1-1.5 1.5h-15A1.5 1.5 0 0 1 3 17.5z",
    alert: "M12 3.5 2.5 20h19zM12 10v4.5M12 17.2v.01",
    key: "M8 20a4.5 4.5 0 1 0 0-9 4.5 4.5 0 0 0 0 9zM11.2 12.3 20 3.5M16 7.5l3 3M13.5 10l2 2",
    check: "M5 12.5l4.5 4.5L19 7.5",
    edit: "M4 20h4L19 9l-4-4L4 16zM13.5 6.5l4 4",
    undo: "M9 14 4 9l5-5M4 9h10.5a5.5 5.5 0 0 1 0 11H11",
    fetch: "M7 17.5a4.5 4.5 0 0 1-.6-8.96A6 6 0 0 1 17.8 8 4 4 0 0 1 18 16M12 11v9M9 17l3 3 3-3",
    clock: "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM12 7v5l3 2",
    plus: "M12 5v14M5 12h14",
    unread: "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18z",
    reading: "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM12 7.5v9M15.5 9v6",
    arrived: "M12 4v10M7.5 9.5l4.5 4.5 4.5-4.5M4 15v5h16v-5",
    play: "M7 4.5v15l12-7.5z",
    arrowRight: "M5 12h14M13 6l6 6-6 6",
    arrowLeft: "M19 12H5M11 6l-6 6 6 6",
    chevronLeft: "M15 6l-6 6 6 6",
    stats: "M5 20v-7M12 20V5M19 20v-11M3 20.5h18",
    chevronRight: "M9 6l6 6-6 6",
    readRtl: "M12 6.5 6.5 12l5.5 5.5M18.5 6.5 13 12l5.5 5.5",
    order: "M10 6h10M10 12h10M10 18h10M4 4.5l1.5-1v5M4 13.5a1.5 1.5 0 1 1 2.6 1L4 17.5h3",
    sidebar: "M4 4.5h16v15H4zM9.5 4.5v15",
    fullSize: "M4 4h16v16H4zM9 9h6v6H9z",
    theme: "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM12 3v18",
    keyboard: "M3 6h18v12H3zM7 10h.01M11 10h.01M15 10h.01M8 14h8",
    storage: "M4 4h16v6H4zM4 14h16v6H4zM8 7h.01M8 17h.01",
    phone: "M7 2.5h10v19H7zM11 18.5h2",
  } as const;
  export type IconName = keyof typeof PATHS;

  // Shapes with lines drawn inside them (a panel divider, the "!" of a warning, a window frame)
  // stay outlines: filled, the inside would vanish.
  const OUTLINE = new Set<string>(["bookmarkOutline", "sidebar", "editPanels", "alert", "maximize", "doubleRtl", "fullSize", "keyboard", "storage", "phone"]);

  // Each icon's pieces, and which are closed shapes to fill (curved ones, like a lens, stay lines).
  const PIECES = Object.fromEntries(
    Object.entries(PATHS).map(([name, d]) => [
      name,
      (d.match(/[Mm][^Mm]*/g) ?? []).map((p) => ({ d: p, fill: !OUTLINE.has(name) && /z\s*$/i.test(p.trim()) && !/a/i.test(p) })),
    ]),
  ) as Record<IconName, { d: string; fill: boolean }[]>;
</script>

<script lang="ts">
  // `stroke` is the line weight the icon was drawn at; solid icons carry half as much again.
  let { name, size = 18, stroke = 1.5 }: { name: IconName; size?: number; stroke?: number } = $props();
  const weight = $derived(name === "more" ? Math.max(2.6, stroke * 1.5 + 1.2) : stroke * 1.5);
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width={weight}
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
>
  {#each PIECES[name] as p, i (i)}
    {#if p.fill}
      <path d={p.d} fill="currentColor" stroke-width={weight * 0.6} />
    {:else}
      <path d={p.d} />
    {/if}
  {/each}
</svg>
