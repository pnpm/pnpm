use std::sync::Arc;

use crate::connection::Conn;
use crate::{Connection, Result};

/// A guard for safely loading SQLite extensions.
///
/// # Example
///
/// ```ignore
/// let _guard = LoadExtensionGuard::new(conn)?;
/// conn.load_extension("uuid", None)?;
/// ```
pub struct LoadExtensionGuard {
    pub(crate) conn: Arc<dyn Conn + Send + Sync>,
}

impl LoadExtensionGuard {
    pub fn new(conn: &Connection) -> Result<LoadExtensionGuard> {
        let conn = conn.conn.clone();
        conn.enable_load_extension(true).map(|_| Self { conn })
    }
}

impl Drop for LoadExtensionGuard {
    fn drop(&mut self) {
        let _ = self.conn.enable_load_extension(false);
    }
}
