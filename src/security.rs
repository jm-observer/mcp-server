use std::path::{Path, PathBuf};
use path_clean::PathClean;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SecurityError {
    #[error("Working directory is not in allowed list: {0}")]
    DirNotAllowed(PathBuf),
    #[error("Sub directory escapes working directory: {0}")]
    PathEscape(String),
}

/// 检查 path 是否在 allowed_dirs 白名单内
pub fn validate_working_dir(path: &Path, allowed_dirs: &[PathBuf]) -> Result<(), SecurityError> {
    let cleaned = path.clean();
    for allowed in allowed_dirs {
        let allowed_cleaned = allowed.clean();
        if cleaned.starts_with(&allowed_cleaned) {
            return Ok(());
        }
    }
    Err(SecurityError::DirNotAllowed(cleaned))
}

/// 检查 sub_dir 解析后的路径是否仍在 working_dir 内（防路径逃逸）
pub fn validate_sub_dir(working_dir: &Path, sub_dir: &str) -> Result<PathBuf, SecurityError> {
    let working_clean = working_dir.clean();
    let combined = working_clean.join(sub_dir).clean();
    
    if combined.starts_with(&working_clean) {
        Ok(combined)
    } else {
        Err(SecurityError::PathEscape(sub_dir.to_string()))
    }
}
