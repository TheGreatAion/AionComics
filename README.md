# AionComics

A desktop comic and manga library and reader. It builds on the ideas behind YACReader: folder-based libraries, Comic Vine metadata, and reading progress. It adds a quieter interface and Guided View, which steps through a page panel by panel.

Built with Tauri 2 (Rust core), SvelteKit 5 and SQLite.

## Running

```sh
npm install
npm run tauri dev      # development
npm run tauri build    # the app, and an installer at src-tauri/target/release/bundle/nsis
```

On first launch, choose your comics folder, or start fresh with a new one. AionComics **only reads that folder**, with exceptions you start yourself: comics and libraries you add are put there (under their own names; nothing is ever renamed, overwritten or moved; see *Adding comics*), and what you choose to delete goes to the Recycle Bin (see *Removing comics*). Its database (`library.db`) and cover thumbnails live in the app data and cache folders (`%APPDATA%\com.aion.comics`, `%LOCALAPPDATA%\com.aion.comics`).

## Library layout

```
<root>/<Collection>/<Series>/[<Section>/…]/<file>.cbz|.cbr
```

- Top-level folders become **collections**. A collection named `Manga` reads right to left.
- Second-level folders are **series**. Deeper folders (`Issues`, `Compendium`, `Annuals`…) become sections on the series page.
- Archives are detected by content, not extension, so mislabeled CBZ/CBR files still open.

If `<root>/.yacreaderlibrary/library.ydb` exists, AionComics reads it on every scan. It imports Comic Vine metadata, page counts, YACReader's cover cache, reading progress whenever YACReader's copy is newer, and how each book reads: a book YACReader marks as left-to-right manga or a webtoon reads left to right, and a webtoon opens scrolling.

**Send my reading to YACReader** (Settings › Library, off until you turn it on) works the other way: the page you're on and the books you finish or mark read or unread are saved into YACReader's library too, so the two apps agree. It's the one thing AionComics writes in the comics folder besides comics you drop in, and it changes only those reading fields, never YACReader's book details or folders. Changes wait while YACReader is open and go once it's closed (and when AionComics closes). A book you opened in YACReader after changing it in AionComics keeps YACReader's reading. Before the first change, a copy of YACReader's database is saved to `yacreader-backup` in the AionComics data folder.

## File types

.cbz, .cbr and .cb7 comics (and .zip, .rar and .7z holding pages), .pdf, drawn by Windows' built-in reader, and .epub, read as its pictures in the order its contents give, so an EPUB novel shows only its illustrations. A .zip, .rar or .7z holding several comics is unpacked when added.

## How each series reads

A series is a Comic (left to right), a Manga (right to left) or a Webtoon (scrolling top to bottom), set from its page's ⋯ menu under *Read as*. Left to *Work it out*: YACReader's mark for a book counts first, then long, narrow pages make it a webtoon, then a library's folder being called Manga makes it manga. A library's ⋯ menu sets all of its series at once, and any added later; one series can still be told otherwise.

## How your folders are read

Any way of keeping comics works, and nothing on disk is rearranged to fit:

- `Marvel / Saga / Saga 001.cbz`: Marvel is a library, Saga a series. Folders under a series are its sections.
- `Saga / Saga 001.cbz`: a folder whose books are named after it, only by number, or all alike is a series. So is one with only numbered folders inside (`Saga / Vol 1 / ...`). These go in a library named after your comics folder.
- `Saga 001.cbz` loose in the comics folder: books sharing a name are one series, in that same library. A book alone is a series of one, and so it goes loose in a library's folder too.
- An empty folder at the top is a library waiting for its first series.

If the guess is wrong, *Settings › Library › Folders in your comics folder* says which they are. It only changes how they show.

## Removing comics

*Remove…* is on a book's right-click menu, a series' ⋯ menu, and a library's ⋯ or right-click menu. **Hide it** leaves the files alone and keeps them out of the app, until you put them back from *Settings › Library*, reading position included. **Delete it**, after asking again, moves the file or folder to the Recycle Bin and forgets the book's details and bookmarks. Nothing outside the comics folder, nor the folder itself, can be deleted.

## Adding comics

- **New library.** The + beside "Library" in the side panel makes a folder at the top of the comics folder.

- **Drop .cbz/.cbr files on the window.** On a series page they're copied into that series' folder. On a collection page each goes into the series of the same name ("Absolute Batman 018.cbz" → `DC/Absolute Batman`), or a new folder named after it. Elsewhere, files go to a series of the same name, if there is one. While you drag, the window shows where each file will go; nothing is copied until you let go. Files keep their own names: AionComics never renames, overwrites, moves or deletes anything in the folder (what a book is called in the app comes from its metadata). A file that's already there is skipped, and so is one whose name is taken by a different file.
- **Or drop them on the side panel.** Let go over a series there to add to it, or over a collection to sort them into its series as on the collection's page. Holding over a folded collection opens it.
- **Folders and bundles work too.** A dropped folder brings every comic inside it (at any depth); books named only by number ("v01.cbz") go to the series the folder is named after. A folder of loose pages becomes one .cbz named after the folder. A .zip or .rar holding several comic files is unpacked, and those books are placed as if dropped one by one; a .zip that's a book itself is copied as it is. The originals are left where they were. A note at the bottom shows how far a big copy has got.
- **Or use a + button.** There's one on each series page (add books to it), on each collection's page and beside an opened collection in the side panel (add a series). Choose comic files or a folder; a box then shows where they'll go, and nothing is copied until you press Add. Adding a series asks for its name, and everything chosen goes into that one series. Comics chosen from several places, or that look like several series, can be kept apart.
- **Or copy them in yourself.** AionComics watches the comics folder and rescans a few seconds after files stop changing, so new books (and removed ones) show up by themselves.
- **Open a comic from Explorer**: AionComics opens a .cbz/.cbr it's started with (Open with → AionComics), in the window that's already open if it's running. An installer built with `npm run tauri build` registers both file types.

## Interface

The art is the interface: monochrome, no accent color. The covers and pages bring all the color. Text is set in Geist; the big titles (series, books, collections) are in Bebas Neue. Both are bundled, so nothing loads from the internet.

- **Home** opens on the page you stopped on, beside the book's title and a Resume button. Below it: *Also reading* (other books opened in the last 30 days), *Next* (the next untouched book in each series you're reading) and the collections as fanned stacks. If nothing was read in the last month, the most recent book you put down takes the top spot.
- **Collections** are in the top bar. Each is one grid of series covers, filterable and sortable by name, recently read or recently added.
- **Series pages** are a gallery of every volume or issue as a big cover (finished ones ticked). Click one and its details (story, credits, characters, Read/Resume) open in a panel under its row; click it again or press Esc to close them. Double-click a cover to read it. Ctrl + the mouse wheel makes the covers bigger or smaller. A series split into formats (issues, collections, omnibus…) has a tab for each.
- Opening a book grows its cover (or the page on Home) into the reader. Closing shrinks it back.
- Ctrl+K searches series and creators. Right-click a book for read/unread, Show in Explorer and its format. On a series page, Ctrl+click or Shift+click picks several books, and right-clicking one of them changes them all (read, unread, format). Marking things read, or taking a book off Continue reading, can be undone for a few seconds (the Undo bar, or Ctrl+Z). Books taken off are listed under Settings › Reading, to put back.
- The arrow keys move between covers and Enter opens the one picked. On Home, ← and → flip the pile and Enter resumes the top book; on a series page ← → step through its books, ↑ ↓ go a row up or down, and Enter reads the one picked.

Covers keep their real proportions: shapes are read from YACReader's cover cache in a background pass after each scan.

## Metadata

A book's details come in layers, each on top of the one before:

1. **The file's own `ComicInfo.xml`** (written by ComicRack, ComicTagger, Mylar and most digital releases), read once per file in a background pass after each scan, and again only if the file changes. It fills whatever the YACReader import leaves empty.
2. **YACReader's database**, imported read-only on every scan: titles, numbers, dates, writer, artists, letterer, cover artist, publisher, synopsis, characters, teams, locations, story arc, genre.
3. **Comic Vine**, fetched by AionComics. On a series page, *Options → Find on Comic Vine* searches for the series (best guess first, ranked by name, start year, issue count and publisher), proposes an issue for each book by number and year, and lets you correct any pair before saving. Credits, characters, teams, locations and story arcs then fill in by themselves in the background. Comic Vine asks apps to stay under about 200 requests an hour per kind of request, so this is throttled (one request a second at most), cached on disk, and resumes after a restart. Add your own free API key in Settings.
4. **Your own edits**: right-click any book → *Edit details…*. Edits always win; ↺ next to an edited field brings back the value underneath.

All of this lives in AionComics' database (`meta_layer`, `cv_series`, `cv_queue`), never in your files, and is reapplied after every scan.

A scan never empties the library: if the library folder is missing (a disconnected drive) or holds no comics, it changes nothing and says so in the top bar; if more than a third of the books vanish at once, they're kept until you reconnect and rescan.

## Reader

Every tool is in the **⋯** menu (or right-click the page), with its shortcut. The top bar keeps Fit, Double and Guided View. **?** or **F1** shows every shortcut, and **Edit** there rebinds any of them.

| Key | Action |
|---|---|
| ← / → | Turn left / right (next is ← in manga) |
| Space, PgDn / PgUp, Backspace | Next / previous (Space scrolls a tall page first) |
| Home / End, G | First / last page, go to a page |
| Ctrl+← / Ctrl+→ | Previous / next book in the series |
| S | Page strip (thumbnails) |
| H / F / P / W | Fit height / width / page / full size |
| Shift+C | Continuous scroll (webtoons) |
| D / J | Double page / double page right to left (cover and wide pages stay single) |
| L / R | Rotate left / right |
| Z | Magnifying glass (mouse wheel changes its strength) |
| + / − / Ctrl+0 | Zoom in / out / reset (Ctrl+wheel too; drag to pan) |
| Ctrl+M / M | Bookmark this page / show bookmarks |
| V | Guided View (the mouse wheel steps through the panels) |
| E | Edit the panels on this page |
| I | Page number and clock |
| C | Options (background, Guided View speed and dimming) |
| O / Ctrl+O | Open a file / a folder of images from anywhere |
| Ctrl+S / Ctrl+E | Save this page / extract a range of pages (never into the library folder) |
| Esc | Back to library |

The reader remembers your fit, scrolling and double page: change them in a book and every book opens that way until you change them again (Settings › Reading shows and sets the same choices). Guided View is remembered separately for manga and comics. Webtoons always open scrolling, and scrolling one doesn't set other books scrolling. At the end of a book the next one in the series is offered; at the end of a series, the top book of your Continue reading pile.

Clicking the left or right third of the screen turns pages; clicking the middle shows the controls. Books opened from outside the library keep no progress, bookmarks or panel edits. Alt+← or the mouse back button goes back in the library.

## Guided View

`src-tauri/src/panels.rs` finds panels by flood-filling the gutter color in from the page edge and treating each enclosed region as a panel. It then orders them with an XY-cut: rows top to bottom, and left to right (right to left for manga) within each row. Results are cached per page in the database. Bump `PANELS_VERSION` in `lib.rs` when the algorithm changes.

A splitting pass then cuts blobs that are really several panels along straight or slanted gutters (ones with paper running between ruled borders, so hair or speed lines don't count). Slanted panels also get their real outline (`poly`), and Guided View dims everything outside it, so a neighbor sharing a diagonal gutter stays dark. Covers, title pages and full-page art (where little of the page is ruled panels) are shown whole.

When the page's art runs off its edges (no gutter around the panels to flood from), a second pass cuts the whole page along straight light or dark lines instead: white gutters, dark gutters, and the black rules modern pages butt panels against.

Reading in Guided View:

- **V** turns it on; arrows, Space and clicks step panel by panel. Options choose whether each new page starts whole or goes straight to its first panel, and whether the art around the panel is dimmed, darkened or hidden.
- **Double-click a panel** to go straight to it (from page view too, which turns Guided View on); double-click it again for the whole page. Because of this, a click to turn waits a fifth of a second to see whether a second click follows; the keyboard doesn't wait.
- **Drag** to pan and **scroll** to zoom when a panel is framed badly; the next step glides back to the panels.
- On a page where no panels were found, the bottom bar says so.

When detection gets a page wrong, press **E** in the reader to fix it: drag boxes to move or resize them, drag on the art to add one, right-click or Delete to remove one, and use *Reading order* to renumber. Edits are saved per page (`user_panels`) and always win over detection; *Reset to detected* drops them.

## Tests

```sh
cd src-tauri
cargo test --lib                                   # unit tests
AION_TEST_ROOT='H:\Comics' cargo test --lib -- --ignored --nocapture
```

The ignored tests run against a real library. They scan it into a temporary database, read CBZ and CBR pages, and write panel-detection overlays to `%TEMP%\aion-panels`.

## Layout

```
src-tauri/src/
  lib.rs       commands, aion:// image protocol, app setup
  scan.rs      folder walk, YACReader import, issue/volume labels
  yacsync.rs   sending your reading back to YACReader (when turned on)
  archive.rs   CBZ/CBR page access, RAR extraction cache, thumbnails
  panels.rs    Guided View panel detection
  db.rs        schema and queries
src/lib/
  views/       Home, Collection, Series, Settings
  components/  top bar, search, covers, book tiles
  flight.ts    cover-to-page shared-element transitions
  reader/      the reader: actions.ts (every tool + key), ctl.svelte.ts (state), page/scroll/guided views, overlays, panel editor
design/        HTML mockups and the clickable prototype (design/prototype)
```
