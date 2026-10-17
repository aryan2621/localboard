use std::path::Path;

use duckdb::{appender_params_from_iter, Connection, OptionalExt};

/// The workspace's embedded DuckDB. Owns relation lifecycle only; profiling and querying live elsewhere.
pub struct Analytics {
    conn: Connection,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelationKind {
    View,
    Table,
}

pub fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

pub fn sql_string(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

impl Analytics {
    pub fn open(path: &Path, memory_limit_gb: u64) -> Result<Self, duckdb::Error> {
        let conn = Connection::open(path)?;
        conn.execute_batch(&format!("SET memory_limit = '{}GB';", memory_limit_gb.max(1)))?;
        Ok(Self { conn })
    }

    /// Wraps a connection that is already set up (for example an encrypted catalog attached and selected).
    pub fn from_connection(conn: Connection, memory_limit_gb: u64) -> Result<Self, duckdb::Error> {
        conn.execute_batch(&format!("SET memory_limit = '{}GB';", memory_limit_gb.max(1)))?;
        Ok(Self { conn })
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    pub fn relation_kind(&self, relation: &str) -> Result<Option<RelationKind>, duckdb::Error> {
        let kind: Option<String> = self
            .conn
            .query_row(
                "SELECT table_type FROM information_schema.tables WHERE table_schema = 'main' AND table_name = ?",
                [relation],
                |row| row.get(0),
            )
            .optional()?;
        Ok(kind.map(|value| if value == "VIEW" { RelationKind::View } else { RelationKind::Table }))
    }

    pub fn relation_exists(&self, relation: &str) -> Result<bool, duckdb::Error> {
        Ok(self.relation_kind(relation)?.is_some())
    }

    /// `reader` is a DuckDB table function call built from trusted, escaped inputs.
    pub fn create_view(&self, relation: &str, reader: &str) -> Result<(), duckdb::Error> {
        let drop = self.drop_statement(relation)?;
        self.atomically(&format!("{drop} CREATE VIEW {} AS SELECT * FROM {reader};", quote_ident(relation)))
    }

    /// Loads all-text rows into a staging table, then swaps it in so readers never see a partial load.
    pub fn replace_text_table<I>(&self, relation: &str, columns: &[String], rows: I) -> Result<(), duckdb::Error>
    where
        I: IntoIterator<Item = Vec<Option<String>>>,
    {
        let staging = format!("{relation}__loading");
        let staging_quoted = quote_ident(&staging);
        let definitions = columns
            .iter()
            .map(|name| format!("{} VARCHAR", quote_ident(name)))
            .collect::<Vec<_>>()
            .join(", ");
        self.conn
            .execute_batch(&format!("DROP TABLE IF EXISTS {staging_quoted}; CREATE TABLE {staging_quoted} ({definitions});"))?;
        let loaded = (|| {
            let mut appender = self.conn.appender(&staging)?;
            for row in rows {
                appender.append_row(appender_params_from_iter(row))?;
            }
            appender.flush()
        })();
        if let Err(error) = loaded {
            let _ = self.conn.execute_batch(&format!("DROP TABLE IF EXISTS {staging_quoted};"));
            return Err(error);
        }
        let drop = self.drop_statement(relation)?;
        self.atomically(&format!("{drop} ALTER TABLE {staging_quoted} RENAME TO {};", quote_ident(relation)))
    }

    pub fn drop_relation(&self, relation: &str) -> Result<(), duckdb::Error> {
        let drop = self.drop_statement(relation)?;
        if drop.is_empty() {
            return Ok(());
        }
        self.conn.execute_batch(&drop)
    }

    pub fn rename_relation(&self, from: &str, to: &str) -> Result<(), duckdb::Error> {
        let keyword = match self.relation_kind(from)? {
            Some(RelationKind::View) => "VIEW",
            Some(RelationKind::Table) => "TABLE",
            None => return Err(duckdb::Error::QueryReturnedNoRows),
        };
        self.conn
            .execute_batch(&format!("ALTER {keyword} {} RENAME TO {};", quote_ident(from), quote_ident(to)))
    }

    fn drop_statement(&self, relation: &str) -> Result<String, duckdb::Error> {
        Ok(match self.relation_kind(relation)? {
            Some(RelationKind::View) => format!("DROP VIEW {};", quote_ident(relation)),
            Some(RelationKind::Table) => format!("DROP TABLE {};", quote_ident(relation)),
            None => String::new(),
        })
    }

    fn atomically(&self, sql: &str) -> Result<(), duckdb::Error> {
        self.conn.execute_batch("BEGIN TRANSACTION;")?;
        match self.conn.execute_batch(sql) {
            Ok(()) => self.conn.execute_batch("COMMIT;"),
            Err(error) => {
                let _ = self.conn.execute_batch("ROLLBACK;");
                Err(error)
            }
        }
    }
}
