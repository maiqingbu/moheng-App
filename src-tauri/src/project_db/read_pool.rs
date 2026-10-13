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
    /// Monotonic release counter per project, used to pick which idle
    /// connection to close when the global cap is reached.
    last_released: HashMap<String, u64>,
    release_tick: u64,
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
            last_released: HashMap::new(),
            release_tick: 0,
        }
    }

    /// Closes one idle connection of the least recently released other project.
    /// Idle connections are not kept around at the expense of a new reader.
    fn evict_idle_except(&mut self, project_id: &str) -> bool {
        let victim = self
            .idle
            .iter()
            .filter(|(id, idle)| id.as_str() != project_id && !idle.is_empty())
            .min_by_key(|(id, _)| self.last_released.get(id.as_str()).copied().unwrap_or(0))
            .map(|(id, _)| id.clone());
        let Some(victim) = victim else {
            return false;
        };
        if let Some(idle) = self.idle.get_mut(&victim) {
            idle.pop();
            if idle.is_empty() {
                self.idle.remove(&victim);
            }
        }
        self.live = self.live.saturating_sub(1);
        true
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
        if self.live >= self.max_total && !self.evict_idle_except(project_id) {
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
        self.release_tick += 1;
        self.last_released
            .insert(project_id.to_string(), self.release_tick);
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

    fn existing_db(root: &Path, id: &str) -> std::path::PathBuf {
        let path = db_path(root, id);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        open_connection(&path, false).unwrap();
        path
    }

    #[test]
    fn fx14_full_pool_evicts_least_recently_released_idle_connection_of_another_project() {
        let temp = tempfile::tempdir().unwrap();
        let (a, b, c) = (
            existing_db(temp.path(), "proj-a"),
            existing_db(temp.path(), "proj-b"),
            existing_db(temp.path(), "proj-c"),
        );
        let mut pool = ReadPool::new(2, 2);
        let first = pool.acquire("proj-a", &a).unwrap();
        pool.release("proj-a", first);
        let second = pool.acquire("proj-b", &b).unwrap();
        pool.release("proj-b", second);
        assert_eq!(pool.live_len(), 2);

        let third = pool.acquire("proj-c", &c).unwrap();

        assert_eq!(pool.live_len(), 2, "the global cap still holds");
        assert_eq!(
            pool.idle_len("proj-a"),
            0,
            "the oldest idle connection is closed"
        );
        assert_eq!(pool.idle_len("proj-b"), 1);
        pool.release("proj-c", third);
        pool.close();
    }

    #[test]
    fn fx14_full_pool_with_only_checked_out_connections_still_reports_exhausted() {
        let temp = tempfile::tempdir().unwrap();
        let (a, b) = (
            existing_db(temp.path(), "proj-a"),
            existing_db(temp.path(), "proj-b"),
        );
        let mut pool = ReadPool::new(1, 1);
        let held = pool.acquire("proj-a", &a).unwrap();
        assert!(matches!(
            pool.acquire("proj-b", &b),
            Err(PoolError::Exhausted)
        ));
        pool.release("proj-a", held);
        pool.close();
    }

    #[test]
    fn fx14_many_projects_read_sequentially_without_exhausting_the_pool() {
        let temp = tempfile::tempdir().unwrap();
        let mut pool = ReadPool::new(16, 4);
        for index in 0..40 {
            let id = format!("proj-{index:02}");
            let path = existing_db(temp.path(), &id);
            let connection = pool.acquire(&id, &path).unwrap();
            pool.release(&id, connection);
            assert!(pool.live_len() <= 16);
        }
        pool.close();
    }
}
