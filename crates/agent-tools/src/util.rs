use std::path::Path;

/// Resolve a path against a working directory.
/// If the path is absolute, return it as-is.
/// Otherwise, join it with the working directory.
pub fn resolve_path(path: &str, working_dir: &str) -> String {
    let p = Path::new(path);
    if p.is_absolute() {
        path.to_string()
    } else {
        Path::new(working_dir).join(path).to_string_lossy().to_string()
    }
}
