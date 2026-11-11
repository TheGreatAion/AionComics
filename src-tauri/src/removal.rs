//! Taking books out of the library, always because you asked, one of two ways: hiding them leaves
//! their files where they are and keeps them out of the app until you put them back; deleting moves
//! their files to the Recycle Bin.

use std::path::Path;

use anyhow::{bail, Result};
use rusqlite::{params, Connection};
use serde::Serialize;

/// What's being taken out.
pub enum Target {
    Books(Vec<i64>),
    Series(i64),
    Library(i64),
}

/// What a target comes to: its books, and where they are in the comics folder (folders where a
/// whole folder goes, otherwise each book's file).
pub struct Found {
    pub books: Vec<i64>,
    pub places: Vec<String>,
    /// Series folders to tidy away if deleting single books leaves them empty.
    pub folders: Vec<String>,
}

fn books_of(conn: &Connection, series: i64) -> Result<Vec<(i64, String)>> {
    let mut st = conn.prepare("SELECT id, rel_path FROM comics WHERE series_id=?1")?;
    let rows = st.query_map([series], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn add_series(conn: &Connection, root: &Path, series: i64, out: &mut Found) -> Result<()> {
    let rel: String = conn.query_row("SELECT rel_path FROM series WHERE id=?1", [series], |r| r.get(0))?;
    let books = books_of(conn, series)?;
    let folder = root.join(&rel).is_dir();
    if folder {
        out.places.push(rel.clone());
    }
    for (id, file) in books {
        out.books.push(id);
        // A loose book that joined a series folder isn't inside it.
        if !folder || !file.starts_with(&format!("{rel}/")) {
            out.places.push(file);
        }
    }
    Ok(())
}

pub fn find(conn: &Connection, root: &Path, target: &Target) -> Result<Found> {
    let mut out = Found { books: Vec::new(), places: Vec::new(), folders: Vec::new() };
    match target {
        Target::Books(ids) => {
            for id in ids {
                let (rel, series): (String, String) = conn.query_row(
                    "SELECT c.rel_path, s.rel_path FROM comics c JOIN series s ON s.id=c.series_id WHERE c.id=?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?;
                out.books.push(*id);
                out.places.push(rel);
                if !out.folders.contains(&series) {
                    out.folders.push(series);
                }
            }
        }
        Target::Series(id) => add_series(conn, root, *id, &mut out)?,
        Target::Library(id) => {
            let (name, at_top): (String, bool) =
                conn.query_row("SELECT name, at_top FROM collections WHERE id=?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))?;
            let series: Vec<i64> = conn
                .prepare("SELECT id FROM series WHERE collection_id=?1")?
                .query_map([id], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            for s in series {
                add_series(conn, root, s, &mut out)?;
            }
            // Its own folder as a whole; the library with no folder of its own is never the comics folder.
            if !at_top && root.join(&name).is_dir() {
                out.places = vec![name];
            }
        }
    }
    Ok(out)
}

/// Forget books: their rows, and everything kept about them except your reading history. A later
/// book could otherwise be given the same number, and with it these books' details.
pub fn forget(conn: &Connection, books: &[i64]) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    forget_now(&tx, books)?;
    tx.commit()?;
    Ok(())
}

const ABOUT_A_BOOK: [&str; 12] = [
    "panels",
    "bookmarks",
    "user_panels",
    "guided_spots",
    "meta_layer",
    "cv_queue",
    "comicinfo_seen",
    "cv_auto",
    "edition_override",
    "reading_hidden",
    "yac_outbox",
    "want_books",
];

fn forget_now(conn: &Connection, books: &[i64]) -> Result<()> {
    for id in books {
        conn.execute("DELETE FROM comics WHERE id=?1", [id])?;
    }
    clear_leftovers(conn)?;
    conn.execute("DELETE FROM series WHERE id NOT IN (SELECT DISTINCT series_id FROM comics)", [])?;
    Ok(())
}

/// Everything kept about books that are gone, except your reading history.
pub fn clear_leftovers(conn: &Connection) -> Result<()> {
    for table in ABOUT_A_BOOK {
        conn.execute(&format!("DELETE FROM {table} WHERE comic_id NOT IN (SELECT id FROM comics)"), [])?;
    }
    // A series with no books left is about to go too.
    conn.execute("DELETE FROM want_series WHERE series_id NOT IN (SELECT DISTINCT series_id FROM comics)", [])?;
    conn.execute("DELETE FROM cv_new WHERE series_id NOT IN (SELECT DISTINCT series_id FROM comics)", [])?;
    Ok(())
}

/// Hide: their files stay, the scan passes over them, and where you'd read to is kept for coming back.
pub fn hide(conn: &Connection, found: &Found, label: &str, now: i64) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    let conn = &*tx;
    for place in &found.places {
        conn.execute("INSERT OR REPLACE INTO hidden(rel_path, label, at) VALUES(?1, ?2, ?3)", params![place, label, now])?;
    }
    for id in &found.books {
        conn.execute(
            "INSERT OR REPLACE INTO hidden_reading(rel_path, read, current_page, last_read_at)
             SELECT rel_path, read, current_page, last_read_at FROM comics WHERE id=?1 AND (read<>0 OR current_page>0)",
            [id],
        )?;
    }
    forget_now(conn, &found.books)?;
    tx.commit()?;
    Ok(())
}

/// Each place's full path, refusing anything that isn't strictly inside the comics folder.
fn checked(root: &Path, place: &str) -> Result<std::path::PathBuf> {
    if place.split('/').any(|p| p.is_empty() || p == "." || p == "..") {
        bail!("Refusing an odd place: {place}");
    }
    let full = root.join(place);
    let real = std::fs::canonicalize(&full)?;
    if real == std::fs::canonicalize(root)? || !real.starts_with(std::fs::canonicalize(root)?) {
        bail!("Refusing to delete anything but what's inside the comics folder");
    }
    Ok(full)
}

/// Delete: each place goes through `bin`, and the books whose files are gone are forgotten, even if
/// a later one couldn't go. The first failure comes back once that's done.
pub fn delete(root: &Path, found: &Found, bin: &mut dyn FnMut(&Path) -> Result<()>) -> Result<()> {
    for place in &found.places {
        let full = match checked(root, place) {
            Ok(full) => full,
            // Already gone is what was wanted.
            Err(_) if !root.join(place).exists() => continue,
            Err(e) => return Err(e),
        };
        bin(&full)?;
    }
    // A series folder left with nothing in it would read as an empty library at the very top. This
    // only ever removes an empty folder.
    for folder in &found.folders {
        let _ = std::fs::remove_dir(crate::intake::as_folder(&root.join(folder)));
    }
    Ok(())
}

/// The books of `found` that really are gone.
pub fn gone(conn: &Connection, root: &Path, found: &Found) -> Result<Vec<i64>> {
    let mut out = Vec::new();
    for id in &found.books {
        let rel: String = conn.query_row("SELECT rel_path FROM comics WHERE id=?1", [id], |r| r.get(0))?;
        if !root.join(rel).exists() {
            out.push(*id);
        }
    }
    Ok(out)
}

/// One thing you've hidden, as Settings lists them.
#[derive(Serialize)]
pub struct Hidden {
    pub label: String,
    pub at: i64,
}

pub fn hidden(conn: &Connection) -> Result<Vec<Hidden>> {
    let mut st = conn.prepare("SELECT label, at FROM hidden GROUP BY label, at ORDER BY at DESC")?;
    let rows = st.query_map([], |r| Ok(Hidden { label: r.get(0)?, at: r.get(1)? }))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn put_back(conn: &Connection, label: &str, at: i64) -> Result<()> {
    conn.execute("DELETE FROM hidden WHERE label=?1 AND at=?2", params![label, at])?;
    Ok(())
}

/// Comics you've just added to somewhere hidden: you want that place back.
pub fn unhide_folder(conn: &Connection, folder: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM hidden WHERE rel_path=?1 OR substr(?1, 1, length(rel_path) + 1) = rel_path || '/'",
        [folder],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup(name: &str) -> (std::path::PathBuf, std::path::PathBuf, Connection) {
        let base = std::env::temp_dir().join(format!("aion-removal-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("Comics");
        for f in ["Marvel/Thor/Thor 001.cbz", "Marvel/Thor/Thor 002.cbz", "Marvel/Thor/cover.txt", "Marvel/Hulk 001.cbz", "Marvel/Hulk 002.cbz", "Saga/Saga 001.cbz", "Maus.cbz"] {
            std::fs::create_dir_all(root.join(f).parent().unwrap()).unwrap();
            std::fs::write(root.join(f), b"PK").unwrap();
        }
        let mut conn = crate::db::open(&base.join("t.db")).unwrap();
        crate::scan::scan(&mut conn, &root).unwrap();
        (base, root, conn)
    }
    fn count(c: &Connection, sql: &str) -> i64 {
        c.query_row(sql, [], |r| r.get(0)).unwrap()
    }
    fn series(c: &Connection, rel: &str) -> i64 {
        c.query_row("SELECT id FROM series WHERE rel_path=?1", [rel], |r| r.get(0)).unwrap()
    }

    #[test]
    fn hiding_leaves_the_files_and_comes_back() {
        let (base, root, mut conn) = setup("hide");
        let thor = series(&conn, "Marvel/Thor");
        let book: i64 = conn.query_row("SELECT id FROM comics WHERE file_name='Thor 001.cbz'", [], |r| r.get(0)).unwrap();
        conn.execute("UPDATE comics SET current_page=7 WHERE id=?1", [book]).unwrap();
        crate::db::toggle_bookmark(&conn, book, 1, 1).unwrap();
        let spot = crate::db::GuidedSpot { page: 7, panel: 2, part: 1 };
        crate::db::set_guided_spot(&conn, book, Some(&spot)).unwrap();
        assert_eq!(crate::db::guided_spot(&conn, book).unwrap(), Some(spot));

        let found = find(&conn, &root, &Target::Series(thor)).unwrap();
        assert_eq!(found.places, ["Marvel/Thor"]);
        hide(&conn, &found, "Thor", 100).unwrap();
        crate::scan::scan(&mut conn, &root).unwrap();
        assert!(root.join("Marvel/Thor/Thor 001.cbz").exists(), "files stay");
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM comics WHERE file_name LIKE 'Thor%'"), 0);
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM bookmarks"), 0);
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM guided_spots"), 0);

        // A new book in the hidden folder stays hidden too.
        std::fs::write(root.join("Marvel/Thor/Thor 003.cbz"), b"PK").unwrap();
        crate::scan::scan(&mut conn, &root).unwrap();
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM comics WHERE file_name LIKE 'Thor%'"), 0);

        let list = hidden(&conn).unwrap();
        assert_eq!((list.len(), list[0].label.as_str()), (1, "Thor"));
        put_back(&conn, "Thor", 100).unwrap();
        crate::scan::scan(&mut conn, &root).unwrap();
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM comics WHERE file_name LIKE 'Thor%'"), 3);
        assert_eq!(count(&conn, "SELECT current_page FROM comics WHERE file_name='Thor 001.cbz'"), 7, "where you'd read to");
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM hidden_reading"), 0);

        // Single books, and adding to a hidden place brings it back.
        let book: i64 = conn.query_row("SELECT id FROM comics WHERE file_name='Thor 001.cbz'", [], |r| r.get(0)).unwrap();
        let one = find(&conn, &root, &Target::Books(vec![book])).unwrap();
        hide(&conn, &one, "Thor #1", 101).unwrap();
        crate::scan::scan(&mut conn, &root).unwrap();
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM comics WHERE file_name='Thor 001.cbz'"), 0);
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM comics WHERE file_name='Thor 002.cbz'"), 1);
        let thor = series(&conn, "Marvel/Thor");
        hide(&conn, &find(&conn, &root, &Target::Series(thor)).unwrap(), "Thor", 102).unwrap();
        unhide_folder(&conn, "Marvel/Thor").unwrap();
        crate::scan::scan(&mut conn, &root).unwrap();
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM comics WHERE file_name='Thor 002.cbz'"), 1);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_hidden_library_goes_whole_and_stays_gone() {
        let (base, root, mut conn) = setup("library");
        let marvel: i64 = conn.query_row("SELECT id FROM collections WHERE name='Marvel'", [], |r| r.get(0)).unwrap();
        let found = find(&conn, &root, &Target::Library(marvel)).unwrap();
        assert_eq!(found.places, ["Marvel"]);
        hide(&conn, &found, "Marvel", 100).unwrap();
        crate::scan::scan(&mut conn, &root).unwrap();
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM collections WHERE name='Marvel'"), 0);
        put_back(&conn, "Marvel", 100).unwrap();
        crate::scan::scan(&mut conn, &root).unwrap();
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM collections WHERE name='Marvel'"), 1);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn deleting_takes_exactly_what_was_asked() {
        let (base, root, mut conn) = setup("delete");
        let mut binned: Vec<String> = Vec::new();
        let bin = |p: &Path, binned: &mut Vec<String>| -> Result<()> {
            binned.push(p.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/"));
            if p.is_dir() { std::fs::remove_dir_all(p)? } else { std::fs::remove_file(p)? }
            Ok(())
        };

        // A series of loose books has no folder: only its own files go.
        let hulk = series(&conn, "Marvel/Hulk");
        let found = find(&conn, &root, &Target::Series(hulk)).unwrap();
        delete(&root, &found, &mut |p| bin(p, &mut binned)).unwrap();
        assert_eq!(binned, ["Marvel/Hulk 001.cbz", "Marvel/Hulk 002.cbz"]);
        forget(&conn, &gone(&conn, &root, &found).unwrap()).unwrap();
        assert!(root.join("Marvel/Thor").exists());

        // A series with a folder goes as that folder.
        binned.clear();
        let thor = series(&conn, "Marvel/Thor");
        let found = find(&conn, &root, &Target::Series(thor)).unwrap();
        delete(&root, &found, &mut |p| bin(p, &mut binned)).unwrap();
        assert_eq!(binned, ["Marvel/Thor"]);
        forget(&conn, &gone(&conn, &root, &found).unwrap()).unwrap();

        // The library with no folder of its own never means the comics folder itself.
        binned.clear();
        let top: i64 = conn.query_row("SELECT id FROM collections WHERE at_top<>0", [], |r| r.get(0)).unwrap();
        let found = find(&conn, &root, &Target::Library(top)).unwrap();
        delete(&root, &found, &mut |p| bin(p, &mut binned)).unwrap();
        binned.sort();
        assert_eq!(binned, ["Maus.cbz", "Saga"]);
        assert!(root.is_dir());

        // Nothing outside the comics folder, or the folder itself.
        std::fs::write(base.join("outside.cbz"), b"PK").unwrap();
        for place in ["../outside.cbz", "", ".", "Marvel/.."] {
            let found = Found { books: vec![], places: vec![place.to_string()], folders: vec![] };
            assert!(delete(&root, &found, &mut |_| panic!("must not get here")).is_err(), "{place}");
        }
        assert!(base.join("outside.cbz").exists());
        forget(&conn, &gone(&conn, &root, &find(&conn, &root, &Target::Library(top)).unwrap()).unwrap()).unwrap();
        crate::scan::scan(&mut conn, &root).unwrap();
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM comics"), 0);
        let _ = std::fs::remove_dir_all(&base);
    }
}
