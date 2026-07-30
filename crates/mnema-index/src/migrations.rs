use rusqlite::Connection;
use rusqlite_migration::{M, Migrations};

use crate::Error;

/// Bumped whenever the DDL changes. Stored in PRAGMA user_version.
pub const SCHEMA_VERSION: i64 = 1;

fn migrations() -> Migrations<'static> {
    Migrations::new(vec![M::up(include_str!("schema.sql"))])
}

/// Takes `&mut` because `rusqlite_migration::to_latest` wraps the whole set in a
/// transaction and so needs exclusive access to the connection.
pub fn apply(conn: &mut Connection) -> Result<(), Error> {
    migrations().to_latest(conn)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::register_vector_extension;

    #[test]
    fn the_migration_set_is_valid() {
        // validate() is not a static check: it opens an in-memory database and
        // runs every migration for real. So the extension has to be registered
        // first — once schema.sql creates a vec0 table, an unregistered run
        // fails with "no such module: vec0".
        register_vector_extension().unwrap();
        migrations().validate().expect("migrations apply cleanly");
    }
}
