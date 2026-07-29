use std::path::Path;

use rusqlite::Connection;
use serde::Serialize;

use crate::{Error, Result};

#[derive(Debug, Clone, Serialize)]
pub struct HistoryEntry {
    pub id: i64,
    pub created_at: String,
    pub app_bundle_id: Option<String>,
    pub raw_text: String,
    pub polished_text: Option<String>,
    pub duration_ms: Option<i64>,
}

pub struct History {
    conn: Connection,
}

impl History {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path).map_err(db_err)?;
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            CREATE TABLE IF NOT EXISTS dictations (
                id INTEGER PRIMARY KEY,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                app_bundle_id TEXT,
                raw_text TEXT NOT NULL,
                polished_text TEXT,
                style TEXT,
                duration_ms INTEGER,
                stt_model TEXT,
                polish_model TEXT
            );
            CREATE VIRTUAL TABLE IF NOT EXISTS dictations_fts USING fts5(
                raw_text, polished_text,
                content='dictations', content_rowid='id',
                tokenize = "trigram case_sensitive 0"
            );
            CREATE TRIGGER IF NOT EXISTS dictations_ai AFTER INSERT ON dictations BEGIN
                INSERT INTO dictations_fts(rowid, raw_text, polished_text)
                VALUES (new.id, new.raw_text, coalesce(new.polished_text, ''));
            END;
            CREATE TRIGGER IF NOT EXISTS dictations_ad AFTER DELETE ON dictations BEGIN
                INSERT INTO dictations_fts(dictations_fts, rowid, raw_text, polished_text)
                VALUES ('delete', old.id, old.raw_text, coalesce(old.polished_text, ''));
            END;
            CREATE TRIGGER IF NOT EXISTS dictations_au AFTER UPDATE ON dictations BEGIN
                INSERT INTO dictations_fts(dictations_fts, rowid, raw_text, polished_text)
                VALUES ('delete', old.id, old.raw_text, coalesce(old.polished_text, ''));
                INSERT INTO dictations_fts(rowid, raw_text, polished_text)
                VALUES (new.id, new.raw_text, coalesce(new.polished_text, ''));
            END;
            "#,
        )
        .map_err(db_err)?;
        Ok(Self { conn })
    }

    pub fn insert_raw(
        &self,
        raw_text: &str,
        app_bundle_id: Option<&str>,
        duration_ms: i64,
        stt_model: &str,
    ) -> Result<i64> {
        self.conn
            .execute(
                "INSERT INTO dictations (raw_text, app_bundle_id, duration_ms, stt_model)
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![raw_text, app_bundle_id, duration_ms, stt_model],
            )
            .map_err(db_err)?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn set_polished(&self, id: i64, polished: &str, polish_model: Option<&str>) -> Result<()> {
        self.conn
            .execute(
                "UPDATE dictations SET polished_text = ?2, polish_model = ?3 WHERE id = ?1",
                rusqlite::params![id, polished, polish_model],
            )
            .map_err(db_err)?;
        Ok(())
    }

    /// Search history. Empty query returns the most recent entries. Queries of
    /// 3+ chars use the trigram FTS index (substring match, RU-friendly);
    /// shorter ones fall back to LIKE.
    pub fn search(&self, query: &str, limit: u32, offset: u32) -> Result<Vec<HistoryEntry>> {
        let query = query.trim();
        let mut entries = Vec::new();

        let mut push = |row: &rusqlite::Row| -> rusqlite::Result<()> {
            entries.push(HistoryEntry {
                id: row.get(0)?,
                created_at: row.get(1)?,
                app_bundle_id: row.get(2)?,
                raw_text: row.get(3)?,
                polished_text: row.get(4)?,
                duration_ms: row.get(5)?,
            });
            Ok(())
        };

        const COLS: &str = "id, created_at, app_bundle_id, raw_text, polished_text, duration_ms";

        if query.is_empty() {
            let mut stmt = self
                .conn
                .prepare(&format!(
                    "SELECT {COLS} FROM dictations ORDER BY id DESC LIMIT ?1 OFFSET ?2"
                ))
                .map_err(db_err)?;
            let mut rows = stmt.query([limit, offset]).map_err(db_err)?;
            while let Some(row) = rows.next().map_err(db_err)? {
                push(row).map_err(db_err)?;
            }
        } else if query.chars().count() >= 3 {
            let mut stmt = self
                .conn
                .prepare(&format!(
                    "SELECT {COLS} FROM dictations WHERE id IN
                       (SELECT rowid FROM dictations_fts WHERE dictations_fts MATCH ?1)
                     ORDER BY id DESC LIMIT ?2 OFFSET ?3"
                ))
                .map_err(db_err)?;
            // Quote the query so FTS operators are treated literally.
            let fts_query = format!("\"{}\"", query.replace('"', ""));
            let mut rows = stmt
                .query(rusqlite::params![fts_query, limit, offset])
                .map_err(db_err)?;
            while let Some(row) = rows.next().map_err(db_err)? {
                push(row).map_err(db_err)?;
            }
        } else {
            let mut stmt = self
                .conn
                .prepare(&format!(
                    "SELECT {COLS} FROM dictations
                     WHERE raw_text LIKE ?1 OR polished_text LIKE ?1
                     ORDER BY id DESC LIMIT ?2 OFFSET ?3"
                ))
                .map_err(db_err)?;
            let like = format!("%{query}%");
            let mut rows = stmt
                .query(rusqlite::params![like, limit, offset])
                .map_err(db_err)?;
            while let Some(row) = rows.next().map_err(db_err)? {
                push(row).map_err(db_err)?;
            }
        }
        Ok(entries)
    }

    pub fn delete(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM dictations WHERE id = ?1", [id])
            .map_err(db_err)?;
        Ok(())
    }

    pub fn count(&self) -> Result<i64> {
        self.conn
            .query_row("SELECT count(*) FROM dictations", [], |r| r.get(0))
            .map_err(db_err)
    }
}

fn db_err(e: rusqlite::Error) -> Error {
    Error::Db(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem() -> History {
        let history = History::open(Path::new(":memory:")).unwrap();
        history
    }

    #[test]
    fn insert_search_russian_morphology() {
        let h = mem();
        let id = h
            .insert_raw("я задеплоил новую версию на прод", Some("com.apple.Notes"), 5000, "turbo")
            .unwrap();
        h.set_polished(id, "Я задеплоил новую версию на прод.", Some("qwen3"))
            .unwrap();

        // Trigram substring: «депло» must match «задеплоил».
        let found = h.search("депло", 10, 0).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].polished_text.as_deref(), Some("Я задеплоил новую версию на прод."));

        // Case-insensitive.
        assert_eq!(h.search("ДЕПЛО", 10, 0).unwrap().len(), 1);
        // Short query falls back to LIKE.
        assert_eq!(h.search("пр", 10, 0).unwrap().len(), 1);
        // Miss.
        assert_eq!(h.search("kubernetes", 10, 0).unwrap().len(), 0);
    }

    #[test]
    fn delete_cleans_fts() {
        let h = mem();
        let id = h.insert_raw("тестовая запись", None, 1000, "m").unwrap();
        h.delete(id).unwrap();
        assert_eq!(h.search("тестовая", 10, 0).unwrap().len(), 0);
        assert_eq!(h.count().unwrap(), 0);
    }
}
