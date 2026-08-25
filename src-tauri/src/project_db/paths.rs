use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct AppPaths {
    root: PathBuf,
}

impl AppPaths {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn catalog_db(&self) -> PathBuf {
        self.root.join("catalog.db")
    }
    pub fn projects_dir(&self) -> PathBuf {
        self.root.join("projects")
    }
    pub fn trash_dir(&self) -> PathBuf {
        self.root.join("trash")
    }
    pub fn project_dir(&self, project_id: &str) -> Result<PathBuf, PathError> {
        if project_id.is_empty()
            || project_id.contains('/')
            || project_id.contains('\\')
            || project_id == "."
            || project_id == ".."
            || project_id.contains("..")
        {
            return Err(PathError::InvalidProjectId);
        }
        let path = self.projects_dir().join(project_id);
        if !path.starts_with(self.projects_dir()) {
            return Err(PathError::OutsideRoot);
        }
        Ok(path)
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PathError {
    #[error("invalid project id")]
    InvalidProjectId,
    #[error("path outside projects root")]
    OutsideRoot,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_path_traversal() {
        assert!(AppPaths::new("/tmp/data").project_dir("../escape").is_err());
    }
}
