# Guided View test kit

Real pages the panel finder is checked against. Only file names and page numbers are kept here,
never the art; the pages are read from the library on this machine (`H:/Comics`).

- `pages.txt`: the pages collected while tuning, many of them hard ones. `fresh.txt`: a random
  sample. One page per line: file, 1 for right-to-left, page number counted from 0.
- `key.jsonl`: the answer key. For each page either the right panels in reading order (`panels`,
  as x, y, width, height in fractions of the page) or, where the finder is still wrong, just the
  right number of stops (`count`).
- `./score.sh`: runs the finder against the key and prints the score and every page it misses.
  Run it before and after any change to `panels.rs` or `frames.rs`.
- `./run.sh <label> [list]`: runs the finder on a whole list and saves pictures of what it found
  in `out/<label>-sheets` (kept out of git). `python compare.py out/a.txt out/b.txt` lists the
  pages that changed between two runs.
- `python key.py right|count ...`: adds checked pages from a run to the key (see the file).
- Panels fixed by hand in the app can join the key: `./fixes.sh` prints them as key lines.
- `AION_CHECK_FLIP=1 ./score.sh` also checks that panels found for one reading direction and
  reordered for the other match panels found for the other (the app keeps them only once).

Each run also notes, per page, the pairs of stops that lie across each other (`overlaps [...]`,
as the share of the smaller one that is covered). A page where they pile up (three such pairs, or two pairs at 30% or more) is shown whole: `jumbled` in the notes.

## Scores

- 2026-11-09, first key (115 pages: 77 from `pages.txt`, 38 from `fresh.txt`): 103 right.
- 2026-11-09, with the jumble rule: 103 right. Of the 378 pages in both lists only the four
  jumbled ones changed, to the whole page (one of them is in the key by its real number of
  panels, so it still counts as a miss, now a harmless one).
