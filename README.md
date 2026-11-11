# AionComics

A desktop comic and manga library and reader. It builds on the ideas behind YACReader: folder-based libraries, Comic Vine metadata, and reading progress. It adds a better interface and Guided View, which steps through a page panel by panel.

Point it at your comics folder and it builds the library: series, covers and details, with Comic Vine filling in the rest. Read a page at a time, two pages side by side, scrolling for webtoons, or one panel at a time with Guided View.

Built with Tauri 2 (Rust core), SvelteKit 5 and SQLite.

## Running

```sh
npm install
npm run tauri dev      # development
npm run tauri build    # the app, and an installer at src-tauri/target/release/bundle/nsis
```

On first launch, choose your comics folder, or start fresh with a new one. AionComics shows what it found there and how it read your folders, so you can correct that before you start.

AionComics **only reads that folder**, with exceptions you start yourself: comics and libraries you add are put there (under their own names; nothing is ever renamed, overwritten or moved; see *Adding comics*), and what you choose to delete goes to the Recycle Bin (see *Removing comics*). Its database (`library.db`), backups and cover thumbnails live in the app data and cache folders (`%APPDATA%\com.aion.comics`, `%LOCALAPPDATA%\com.aion.comics`).

## File types

.cbz, .cbr and .cb7 comics (and .zip, .rar and .7z holding pages), .pdf, drawn by Windows' built-in reader, and .epub, read as its pictures in the order its contents give, so an EPUB novel shows only its illustrations. Archives are recognized by what's inside them, not their extension, so a mislabeled CBZ or CBR still opens. A .zip, .rar or .7z holding several comics is unpacked when added.

## How your folders are read

Any way of keeping comics works, and nothing on disk is rearranged to fit:

- `Marvel / Saga / Saga 001.cbz`: Marvel is a library, Saga a series. Folders under a series (`Issues`, `Compendium`, `Annuals`…) are its formats.
- `Saga / Saga 001.cbz`: a folder whose books are named after it, only by number, or all alike is a series. So is one with only numbered folders inside (`Saga / Vol 1 / ...`). These go in a library named after your comics folder.
- `Saga 001.cbz` loose in the comics folder: books sharing a name are one series, in that same library. A book alone is a series of one, and so it goes loose in a library's folder too.
- An empty folder at the top is a library waiting for its first series.

If the guess is wrong, *Settings › Library › Folders in your comics folder* says which they are. It only changes how they show.

Moving or renaming a book or folder in Explorer keeps its place and details, when there's exactly one book it could be: the same name and size, or for a big file the same size and date.

A scan never empties the library: if the comics folder is missing (a disconnected drive) or holds no comics, it changes nothing and says so in the top bar; if more than a third of the books vanish at once, they're kept until you reconnect and rescan.

## How each series reads

A series is a Comic (left to right), a Manga (right to left) or a Webtoon (scrolling top to bottom), set from its page's ⋯ menu under *Read as*. Left to *Work it out*: YACReader's mark for a book counts first, then long, narrow pages make it a webtoon, then a library folder called Manga makes it manga. A library's ⋯ menu sets all of its series at once, and any added later; one series can still be told otherwise.

## Getting around

- **The side panel** (Ctrl+B shows and hides it) is the way around the app: Home, the books you're partway through, the series added lately, Want to read, and every library opening into its series, with a filter over all of them. While it's hidden, the top bar has a Want to read button too. The top bar keeps back, search, your reading and Settings.
- **Home** fits the window, and only scrolls if you have more libraries than one row holds. It opens on the page you stopped on, over that book's cover printed in its own color, with the writer, the page you're on, how far through you are and how long the rest should take at your pace. The other books you're reading are piled behind it: flip through them with the arrows under the book, ← and →, or the mouse wheel, and Enter resumes the one on top. Below are your libraries, each a fan of covers with its counts. If nothing was read in the last month, the books you most recently put down take the top spot.
- **Library pages** are one grid of series covers, each with a bar showing how much you've read. Filter by name, sort by A to Z, recently read or recently added, and show only series with books still unread, or only ones you've started and not finished. The headline is spelled out in letters cut from the library's own covers.
- **Series pages** are a gallery of every volume or issue as a big cover (finished ones ticked). Click one and its details (story, credits, characters, Read or Resume) open under its row; click it again or press Esc to close them. Double-click a cover to read it. Ctrl + the mouse wheel makes the covers bigger or smaller. A series split into formats (issues, collections, omnibus…) has a tab for each; drag the tabs to reorder them, and the series opens on the one in front. A round button shows only what's still unread.
- **Creators, characters, teams and story arcs** each have a page: click a name in a book's details to see everything in your library by that creator or with that character, one row per series.
- **Search** (Ctrl+K) covers series, books (by title or file name), creators, characters and teams, and story arcs, closest matches first, with the closest series shown large and where you are in it. Every result can be reached by scrolling; the arrow keys move around them and Enter opens one.
- **The keyboard** works everywhere: the arrow keys move between covers and Enter opens the one picked; on a series page ↑ ↓ go a row up or down. Alt+← or the mouse back button goes back.
- Opening a book grows its cover (or the page on Home) into the reader, and closing shrinks it back. The window comes back the size and place you left it, or maximized, unless that screen has gone.

Right-click a book for read or unread, *Mark earlier ones as read*, Want to read, its format, *Edit details…*, *Find on Comic Vine…*, Show in Explorer and *Remove…*. On a series page, Ctrl+click or Shift+click picks several books, and right-clicking one of them changes them all. Marking things read, or taking a book off Continue reading or Want to read, can be undone for a few seconds (the Undo bar, or Ctrl+Z). Books taken off Continue reading are listed under *Settings › Reading*, to put back.

## Want to read

Put a series on your list with the bookmark button on its page, or from its ⋯ or right-click menu. A single book (an omnibus, an annual, one volume of a long series) goes on from its right-click menu with *Want to read this book*. The list has its own page, first under Library in the side panel, laid out like a library: sort it by when things went on or A to Z, and use + there to find and add series and books without leaving the page. Covers on library pages wear a ribbon while they're on the list.

Something stays on the list until you start it (a page read or a book marked read after you put it on) or until it's all read. Then it moves to Continue reading.

## Your reading

The rising-line button in the top bar shows what your reading adds up to: pages read, your streak, pages per week, the series you've read most and the books you've finished. It's recorded from the first sitting on; nothing is guessed for the time before.

## How formats fit together

Finishing a book, by reading to its last page or marking it read, also marks what it holds: the issues a collection says it collects, the chapters a manga volume lists, a smaller collection inside a bigger one. Finishing everything in a collection marks the collection. It goes by the book's description, and only when that's about this series ("Collects Superman #1-5" in Action Comics counts for nothing), so a book that doesn't say links to nothing. Marking a book unread changes only that book, and Undo takes back everything a mark changed. Books read before this came in stay as they were until one of them is finished again.

When the story runs across formats, a series gets an *In order* tab: its collections and then the issues or chapters that carry on past them, with annuals and specials placed by the date they came out (each is labeled "after #12"). At the end of a book in that order, the next one in it is offered. Annuals with no date stay at the end. A series named as a reading order ("019 Siege - Storming Asgard") is already in order and is left alone.

## Adding comics

- **New library.** The + beside "Library" in the side panel makes a folder at the top of the comics folder.
- **Drop comic files on the window.** On a series page they're copied into that series' folder. On a library page each goes into the series of the same name ("Absolute Batman 018.cbz" → `DC/Absolute Batman`), or a new folder named after it. Elsewhere, files go to a series of the same name, if there is one. While you drag, the window shows where each file will go; nothing is copied until you let go. Files keep their own names: AionComics never renames, overwrites, moves or deletes anything in the folder (what a book is called in the app comes from its details). A file that's already there is skipped, and so is one whose name is taken by a different file.
- **Or drop them on the side panel.** Let go over a series there to add to it, or over a library to sort them into its series as on the library's page. Holding over a folded library opens it.
- **Folders and bundles work too.** A dropped folder brings every comic inside it (at any depth); books named only by number ("v01.cbz") go to the series the folder is named after. A folder of loose pages becomes one .cbz named after the folder. A .zip or .rar holding several comic files is unpacked, and those books are placed as if dropped one by one; a .zip that's a book itself is copied as it is. The originals are left where they were. A note at the bottom shows how far a big copy has got.
- **Or use a + button.** There's one on each series page (add books to it), on each library's page, beside each library on Home and beside an opened library in the side panel (add a series). Choose comic files or a folder; a box then shows where they'll go, and nothing is copied until you press Add. Adding a series asks for its name, and everything chosen goes into that one series. Comics chosen from several places, or that look like several series, can be kept apart.
- **Or copy them in yourself.** AionComics watches the comics folder and adds new books (and drops removed ones) a few seconds after files stop changing. *Settings › Library › Watch for new comics* turns this off.
- **Open a comic from Explorer**: AionComics opens a comic it's started with (Open with → AionComics), in the window that's already open if it's running. The installer registers .cbz, .cbr and .cb7.

## Removing comics

*Remove…* is on a book's right-click menu, a series' ⋯ menu, and a library's ⋯ or right-click menu. **Hide it** leaves the files alone and keeps them out of the app, until you put them back from *Settings › Library*, reading position included. **Delete it**, after asking again, moves the file or folder to the Recycle Bin and forgets the book's details and bookmarks. Nothing outside the comics folder, nor the folder itself, can be deleted.

*Settings › Library › Duplicates* lists books that may be in your library twice. AionComics deletes nothing there: each copy can be shown in Explorer to deal with yourself, and a group that isn't really duplicates can be hidden.

## Details

A book's details come in layers, each on top of the one before:

1. **The file's own `ComicInfo.xml`** (written by ComicRack, ComicTagger, Mylar and most digital releases), read once per file in the background after each scan, and again only if the file changes. It fills whatever the YACReader import leaves empty. *Settings › Details* can turn this off.
2. **YACReader's database**, imported read-only on every scan: titles, numbers, dates, writer, artists, letterer, cover artist, publisher, synopsis, characters, teams, locations, story arc, genre.
3. **Comic Vine**, fetched by AionComics. On a series page, *Find on Comic Vine…* in the ⋯ menu searches for the series (best guess first, ranked by name, start year, issue count and publisher), proposes an issue for each book by number and year, and lets you correct any pair before saving. A single book can be matched on its own from its right-click menu. Credits, characters, teams, locations and story arcs then fill in by themselves in the background. Comic Vine asks apps to stay under about 200 requests an hour per kind of request, so this is throttled (one request a second at most), cached on disk, and picks up again after a restart. Add your own free API key in *Settings › Details*.
4. **Your own edits**: right-click any book → *Edit details…*. Edits always win; ↺ next to an edited field brings back the value underneath.

All of this lives in AionComics' database, never in your files, and is reapplied after every scan.

## YACReader

If `<comics folder>/.yacreaderlibrary/library.ydb` exists, AionComics reads it on every scan. It imports Comic Vine details, page counts, YACReader's cover cache, reading progress whenever YACReader's copy is newer, and how each book reads: a book YACReader marks as left-to-right manga or a webtoon reads left to right, and a webtoon opens scrolling.

**Send my reading to YACReader** (*Settings › Library*, off until you turn it on) works the other way: the page you're on and the books you finish or mark read or unread are saved into YACReader's library too, so the two apps agree. It's the one thing AionComics writes in the comics folder besides comics you add, and it changes only those reading fields, never YACReader's book details or folders. Changes wait while YACReader is open and go once it's closed (and when AionComics closes). A book you opened in YACReader after changing it in AionComics keeps YACReader's reading. Before the first change, a copy of YACReader's database is saved to `yacreader-backup` in the AionComics data folder.

## Reader

The toolbar fades away while you read. It has the page strip, fit, double page, Guided View, bookmark and options, plus **⋯** for every tool (or right-click the page), each with its shortcut. Type a page number in the box at the bottom and press Enter to go there. **?** or **F1** shows every shortcut, and **Edit** there changes any of them (so does *Settings › Keyboard*).

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
| I | Keep the page number and clock showing |
| F11, Alt+Enter | Full screen |
| C | Options (brightness and contrast, background, page fit, Guided View speed and what shows around the panel) |
| O / Ctrl+O | Open a file / a folder of images from anywhere |
| Ctrl+S / Ctrl+E | Save this page / extract a range of pages (never into the comics folder) |
| Esc | Back to library |

The reader remembers your fit, scrolling and double page: change them in a book and every book opens that way until you change them again (*Settings › Reading* shows and sets the same choices). Guided View is remembered separately for manga and comics. Webtoons always open scrolling, and scrolling one doesn't set other books scrolling. At the end of a book the next one in the series is offered; at the end of a series, the top book of your Continue reading pile.

Clicking the left or right third of the screen turns pages; clicking the middle shows the controls. Pages near the one you're on are prepared in the background, so turning and jumping don't wait. Books opened from outside the library keep no progress, bookmarks or panel edits.

A book that won't open says why in plain words (the drive isn't connected, the file has gone, or it's damaged), and offers the next book, another try, the file in Explorer, or the way back.

## Guided View

Guided View steps through a page one panel at a time.

- **V** turns it on; arrows, Space and clicks step panel by panel. *Settings › Guided View* (or **C** in the reader) chooses whether each new page starts whole or goes straight to its first panel, whether tall panels are read top to bottom closer up, and whether the art around the panel is dimmed, darkened or hidden.
- **Double-click a panel** to go straight to it (from page view too, which turns Guided View on); double-click it again for the whole page. Because of this, a click to turn waits a fifth of a second to see whether a second click follows; the keyboard doesn't wait.
- **Drag** to pan and **scroll** to zoom when a panel is framed badly; the next step glides back to the panels.
- Covers, title pages and full-page art are shown whole. On a page where no panels were found, the bottom bar says so.

When a page comes out wrong, press **E** to fix it: drag boxes to move or resize them, drag on the art to add one, right-click or Delete to remove one, and use *Reading order* to renumber. Your fixes are saved per page and always win over what was found; *Reset to detected* drops them.

How panels are found (`src-tauri/src/panels.rs`, `frames.rs`), in three passes:

1. **Gutters.** The gutter color is flood-filled in from the page edge, and each enclosed region is a panel. A splitting pass then cuts regions that are really several panels along straight or slanted gutters (ones with paper running between ruled borders, so hair or speed lines don't count). Slanted panels also get their real outline, and Guided View dims everything outside it, so a neighbor sharing a diagonal gutter stays dark.
2. **Lines.** When the art runs off the page's edges (no gutter around the panels to flood from), the page is cut along straight light or dark lines instead: white gutters, dark gutters, and the black rules modern pages butt panels against.
3. **Drawn frames.** When neither finds anything, panels are found by the borders drawn around them, which catches panels laid over each other and framed panels set over art that fills the page.

Panels are then put in reading order with an XY-cut: rows top to bottom, and left to right (right to left for manga) within each row. Results are cached per page in the database; bump `PANELS_VERSION` in `lib.rs` when the method changes.

## Settings

- **Look**: dark, light or matching Windows; how much screens and covers move; the printed cover behind library names.
- **Reading**: page brightness and contrast, fit, two pages side by side, manga pairs right to left, scrolling, what's behind the page, the page number and clock, and books taken off Continue reading.
- **Guided View**: start books in it, how it moves from panel to panel, what shows around the panel, tall panels.
- **Keyboard**: every reader shortcut, changeable.
- **Library**: the comics folder and how its folders are read, hidden books, watching for new comics, sending your reading to YACReader, and duplicates.
- **Details**: your Comic Vine key, and reading the details stored in the files.
- **Storage**: the page previews kept so books open instantly, and whether to keep them for books you haven't opened.
- **Backups**: a copy of the database (reading progress, bookmarks, panel fixes, Comic Vine matches and your edits) is made once a day while AionComics runs, and the newest few are kept. Restoring one restarts the app with it.

## Interface

The art is the interface: monochrome, no accent color, so the covers and pages bring all the color. Text is set in Geist; big titles and caption boxes are in Bebas Neue; Geist Mono is kept for real codes like keys, file paths and web addresses. All three are bundled, so nothing loads from the internet. Covers keep their real proportions.

## Tests

```sh
cd src-tauri
cargo test --lib                                   # unit tests
AION_TEST_ROOT='H:\Comics' cargo test --lib -- --ignored --nocapture
```

The ignored tests run against a real library. They scan it into a temporary database, read CBZ and CBR pages, and write panel-detection overlays to `%TEMP%\aion-panels`. Setting `AION_TEST_HOME` to an empty folder keeps all of the app's data there, for trying a fresh start without touching your own.

## Layout

```
src-tauri/src/
  lib.rs        commands, aion:// image protocol, app setup
  db.rs         schema and queries
  scan.rs       folder walk, YACReader import, issue and volume labels
  intake.rs     watching the comics folder, and copying in added comics
  removal.rs    hiding and deleting books
  archive.rs    CBZ/CBR/CB7/EPUB page access, RAR extraction cache, thumbnails
  rarindex.rs   reading any page of a RAR directly
  pdf.rs        PDF pages, from Windows' own reader
  comicinfo.rs  details stored inside comic files
  cv.rs         Comic Vine search, matching and background fill-in
  editions.rs   a book's format, what it holds, and the story order
  panels.rs     Guided View panel detection (gutters and lines)
  frames.rs     Guided View panel detection (drawn frames)
  warm.rs       preparing pages and panels ahead of the reader
  stats.rs      reading sessions and what they add up to
  backup.rs     daily database backups and restoring them
  yacsync.rs    sending your reading back to YACReader (when turned on)
  window.rs     the window's size and place, for next time
src/lib/
  views/        Home, library, series, Want to read, people, stats, duplicates, settings, welcome
  components/   side panel, top bar, search, covers, menus, Comic Vine matcher, details editor
  reader/       the reader: actions.ts (every tool and key), ctl.svelte.ts (state), page/scroll/guided views, overlays, panel editor
  flight.ts     cover-to-page transitions
design/         HTML mockups
```
