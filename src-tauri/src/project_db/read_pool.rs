use crate::project_db::connection::{open_connection, DatabaseError};
use libsql_rusqlite::Connection;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum PoolError {
    #[error("no read connection available: total pool exhausted")]
    Exhausted,
    #[error("no read connection available: per-project bound reached")]
    PerProjectExhausted,
    #[error("database: {0}")]
    Database(#[from] DatabaseError),
}

/// Bounded, reusable, project-scoped pool of read-only SQLite connections.
///
/// The pool enforces a global cap on live connections and a per-project cap so
/// one project cannot starve others. Connections are borrowed to the caller and
/// returned via [`ReadPool::release`], which reuses them for later acquires of
/// the same project instead of opening a new handle.
pub struct ReadPool {
    max_total: usize,
    max_per_project: usize,
    idle: HashMap<String, Vec<Connection>>,
    checked_out: HashMap<String, usize>,
    live: usize,
}

impl ReadPool {
    /// Creates an empty pool. `max_total` bounds live connections across all
    /// projects; `max_per_project` bounds how many one project may hold at once.
    pub fn new(max_total: usize, max_per_project: usize) -> Self {
        Self {
            max_total: max_total.max(1),
            max_per_project: max_per_project.max(1),
            idle: HashMap::new(),
            checked_out: HashMap::new(),
            live: 0,
        }
    }

    fn project_checked_out(&self, project_id: &str) -> usize {
        self.checked_out.get(project_id).copied().unwrap_or(0)
    }

    /// Borrows a read-only connection for `project_id`, opening a new one only
    /// when no idle connection is available and both bounds permit it.
    pub fn acquire(&mut self, project_id: &str, database: &Path) -> Result<Connection, PoolError> {
        if let Some(idle) = self.idle.get_mut(project_id) {
            if let Some(connection) = idle.pop() {
                *self.checked_out.entry(project_id.to_string()).or_default() += 1;
                return Ok(connection);
            }
        }
        if self.live >= self.max_total {
            return Err(PoolError::Exhausted);
        }
        if self.project_checked_out(project_id) >= self.max_per_project {
            return Err(PoolError::PerProjectExhausted);
        }
        let connection = open_connection(database, true)?;
        self.live += 1;
        *self.checked_out.entry(project_id.to_string()).or_default() += 1;
        Ok(connection)
    }

    /// Returns a borrowed connection so it can be reused by a later acquire.
    pub fn release(&mut self, project_id: &str, connection: Connection) {
        if let Some(count) = self.checked_out.get_mut(project_id) {
            if *count > 0 {
                *count -= 1;
            }
        }
        self.idle
            .entry(project_id.to_string())
            .or_default()
            .push(connection);
    }

    /// Number of idle connections currently pooled (available for reuse).
    pub fn idle_len(&self, project_id: &str) -> usize {
        self.idle.get(project_id).map_or(0, Vec::len)
    }

    pub fn checked_out_len(&self, project_id: &str) -> usize {
        self.project_checked_out(project_id)
    }

    /// Invalidates only one project's idle connections before a database file
    /// replacement. Checked-out leases are intentionally not silently killed;
    /// the caller must either wait for them to drain or return a blocking error.
    pub fn invalidate_project(&mut self, project_id: &str) {
        if let Some(idle) = self.idle.remove(project_id) {
            self.live = self.live.saturating_sub(idle.len());
        }
    }

    /// Live connections across the whole pool (idle + checked out).
    pub fn live_len(&self) -> usize {
        self.live
    }

    /// Drops every pooled (idle) connection. Connections still borrowed are left
    /// with the caller; the pool no longer counts them once they are released.
    pub fn close(&mut self) {
        self.idle.clear();
        self.live = self.checked_out.values().sum();
    }

    /// Explicit release of all remaining bookkeeping; no pooled connections remain.
    pub fn close_and_release_all(&mut self) {
        self.idle.clear();
        self.checked_out.clear();
        self.live = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_db::paths::AppPaths;
    use std::fs;

    fn db_path(root: &Path, id: &str) -> std::path::PathBuf {
        AppPaths::new(root)
            .project_dir(id)
            .unwrap()
            .join("project.db")
    }

    #[test]
    fn pool_reuses_released_connection_instead_of_opening_new() {
        let temp = tempfile::tempdir().unwrap();
        let path = db_path(temp.path(), "proj-a");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        {
            // Open and drop a writable connection so the file exists.
            open_connection(&path, false).unwrap();
        }
        let mut pool = ReadPool::new(4, 2);
        let first = pool.acquire("proj-a", &path).unwrap();
        assert_eq!(pool.live_len(), 1);
        pool.release("proj-a", first);
        assert_eq!(pool.idle_len("proj-a"), 1);

        let second = pool.acquire("proj-a", &path).unwrap();
        // Idle connection reused -> no new live connection opened.
        assert_eq!(pool.live_len(), 1);
        assert_eq!(pool.idle_len("proj-a"), 0);
        pool.release("proj-a", second);
        pool.close();
    }

    #[test]
    fn pool_obeys_global_cap() {
        let temp = tempfile::tempdir().unwrap();
        let path = db_path(temp.path(), "proj-a");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        open_connection(&path, false).unwrap();
        let mut pool = ReadPool::new(2, 5);
        let c1 = pool.acquire("proj-a", &path).unwrap();
        let c2 = pool.acquire("proj-a", &path).unwrap();
        assert_eq!(pool.live_len(), 2);
        let exhausted = pool.acquire("proj-a", &path);
        assert!(matches!(exhausted, Err(PoolError::Exhausted)));
        drop(c1);
        drop(c2);
        pool.close();
    }

    #[test]
    fn pool_obeys_per_project_isolation_bound() {
        let temp = tempfile::tempdir().unwrap();
        let path_a = db_path(temp.path(), "proj-a");
        let path_b = db_path(temp.path(), "proj-b");
        fs::create_dir_all(path_a.parent().unwrap()).unwrap();
        fs::create_dir_all(path_b.parent().unwrap()).unwrap();
        open_connection(&path_a, false).unwrap();
        open_connection(&path_b, false).unwrap();
        let mut pool = ReadPool::new(10, 1);
        let a1 = pool.acquire("proj-a", &path_a).unwrap();
        // A single project cannot exceed its per-project bound, but another
        // project can still acquire its own connection.
        let too_many = pool.acquire("proj-a", &path_a);
        assert!(matches!(too_many, Err(PoolError::PerProjectExhausted)));
        let b1 = pool.acquire("proj-b", &path_b).unwrap();
        assert_eq!(pool.live_len(), 2);
        pool.release("proj-a", a1);
        pool.release("proj-b", b1);
        pool.close();
    }
}
