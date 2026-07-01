use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SnapshotPatch {
    pub hash: String,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SnapshotDiffStatus {
    Added,
    Deleted,
    Modified,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SnapshotFileDiff {
    pub file: String,
    pub patch: String,
    pub additions: u32,
    pub deletions: u32,
    pub status: Option<SnapshotDiffStatus>,
}

pub struct SnapshotManager {
    gitdir: PathBuf,
    worktree: PathBuf,
    lock: tokio::sync::Mutex<()>,
}

fn worktree_hash(path: &Path) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    path.to_string_lossy().hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

async fn git_run(
    gitdir: &Path,
    worktree: &Path,
    args: &[&str],
) -> anyhow::Result<(i32, String, String)> {
    let output = tokio::process::Command::new("git")
        .arg("--git-dir")
        .arg(gitdir)
        .arg("--work-tree")
        .arg(worktree)
        .args(args)
        .output()
        .await?;
    Ok((
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    ))
}

impl SnapshotManager {
    pub fn for_worktree(worktree: &Path) -> Self {
        let hash = worktree_hash(worktree);
        let base = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".agent")
            .join("snapshots")
            .join(&hash);
        Self {
            gitdir: base.join(".git"),
            worktree: worktree.to_path_buf(),
            lock: tokio::sync::Mutex::new(()),
        }
    }

    pub async fn init(&self) -> Result<()> {
        let _guard = self.lock.lock().await;
        tokio::fs::create_dir_all(&self.gitdir)
            .await
            .context("creating gitdir")?;
        tokio::fs::create_dir_all(&self.worktree)
            .await
            .context("creating worktree")?;

        let (code, stdout, stderr) =
            git_run(&self.gitdir, &self.worktree, &["init"]).await?;
        if code != 0 {
            anyhow::bail!("git init failed ({}): {} {}", code, stdout, stderr);
        }

        let (_, _, _) = git_run(&self.gitdir, &self.worktree, &[
            "config", "core.autocrlf", "false",
        ])
        .await?;
        let (_, _, _) = git_run(&self.gitdir, &self.worktree, &[
            "config", "core.quotepath", "false",
        ])
        .await?;

        let gitignore = self.worktree.join(".gitignore");
        if !gitignore.exists() {
            tokio::fs::write(
                &gitignore,
                "*.exe\n*.dll\n*.so\n*.dylib\n*.bin\n*.o\n*.a\n*.class\n*.jar\n*.pyc\n*.pyo\n*.wasm\nnode_modules/\n.DS_Store\n",
            )
            .await
            .context("writing .gitignore")?;
        }

        Ok(())
    }

    pub async fn track(&self) -> Result<String> {
        let _guard = self.lock.lock().await;

        let (code, stdout, stderr) =
            git_run(&self.gitdir, &self.worktree, &["add", "-A"]).await?;
        if code != 0 {
            anyhow::bail!("git add failed ({}): {} {}", code, stdout, stderr);
        }

        let (code, stdout, stderr) = git_run(
            &self.gitdir,
            &self.worktree,
            &["commit", "--allow-empty", "-m", "snapshot"],
        )
        .await?;
        if code != 0 {
            anyhow::bail!("git commit failed ({}): {} {}", code, stdout, stderr);
        }

        let (code, stdout, stderr) = git_run(
            &self.gitdir,
            &self.worktree,
            &["rev-parse", "HEAD"],
        )
        .await?;
        if code != 0 {
            anyhow::bail!("rev-parse failed ({}): {} {}", code, stdout, stderr);
        }

        Ok(stdout.trim().to_string())
    }

    pub async fn patch(&self, since_hash: &str) -> Result<Vec<String>> {
        let _guard = self.lock.lock().await;
        let range = format!("{}..HEAD", since_hash);
        let (code, stdout, stderr) = git_run(
            &self.gitdir,
            &self.worktree,
            &["diff", "--name-only", &range],
        )
        .await?;
        if code != 0 {
            anyhow::bail!("git diff failed ({}): {} {}", code, stdout, stderr);
        }
        Ok(stdout
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| l.to_string())
            .collect())
    }

    pub async fn diff(&self, since_hash: &str) -> Result<String> {
        let _guard = self.lock.lock().await;
        let range = format!("{}..HEAD", since_hash);
        let (code, stdout, stderr) = git_run(
            &self.gitdir,
            &self.worktree,
            &["diff", &range],
        )
        .await?;
        if code != 0 {
            anyhow::bail!("git diff failed ({}): {} {}", code, stdout, stderr);
        }
        Ok(stdout)
    }

    pub async fn diff_full(&self, from: &str, to: &str) -> Result<Vec<SnapshotFileDiff>> {
        let _guard = self.lock.lock().await;
        let range = format!("{}..{}", from, to);

        let (code, stdout, stderr) = git_run(
            &self.gitdir,
            &self.worktree,
            &["diff", "--numstat", &range],
        )
        .await?;
        if code != 0 {
            anyhow::bail!("git diff --numstat failed ({}): {} {}", code, stdout, stderr);
        }

        let (code, diff_stdout, stderr) = git_run(
            &self.gitdir,
            &self.worktree,
            &["diff", &range],
        )
        .await?;
        if code != 0 {
            anyhow::bail!("git diff failed ({}): {} {}", code, diff_stdout, stderr);
        }

        let (code, name_status_stdout, stderr) = git_run(
            &self.gitdir,
            &self.worktree,
            &["diff", "--name-status", &range],
        )
        .await?;
        if code != 0 {
            anyhow::bail!(
                "git diff --name-status failed ({}): {} {}",
                code,
                name_status_stdout,
                stderr
            );
        }

        let mut status_map = std::collections::HashMap::new();
        for line in name_status_stdout.lines() {
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.splitn(3, '\t').collect();
            if parts.len() >= 2 {
                let status = match parts[0].chars().next() {
                    Some('A') => Some(SnapshotDiffStatus::Added),
                    Some('D') => Some(SnapshotDiffStatus::Deleted),
                    Some('M') => Some(SnapshotDiffStatus::Modified),
                    _ => None,
                };
                status_map.insert(parts[parts.len() - 1].to_string(), status);
            }
        }

        let mut per_file_patches: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        let mut current_file: Option<String> = None;
        let mut current_patch = String::new();
        for line in diff_stdout.lines() {
            if line.starts_with("diff --git") {
                if let Some(f) = current_file.take() {
                    per_file_patches.insert(f, current_patch.clone());
                }
                current_patch.clear();
                current_patch.push_str(line);
                current_patch.push('\n');
            } else if line.starts_with("+++") || line.starts_with("---") {
                if line.starts_with("+++") {
                    let path = line
                        .strip_prefix("+++ b/")
                        .or_else(|| line.strip_prefix("+++ "))
                        .unwrap_or("");
                    if current_file.is_none() || path.starts_with("b/") {
                        current_file = Some(
                            path.strip_prefix("b/")
                                .unwrap_or(path)
                                .to_string(),
                        );
                    }
                }
                current_patch.push_str(line);
                current_patch.push('\n');
            } else {
                current_patch.push_str(line);
                current_patch.push('\n');
            }
        }
        if let Some(f) = current_file {
            per_file_patches.insert(f, current_patch);
        }

        let mut result = Vec::new();
        for line in stdout.lines() {
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.splitn(3, '\t').collect();
            if parts.len() < 3 {
                continue;
            }
            let additions: u32 = parts[0].parse().unwrap_or(0);
            let deletions: u32 = parts[1].parse().unwrap_or(0);
            let file = parts[2].to_string();
            let patch = per_file_patches
                .get(&file)
                .cloned()
                .unwrap_or_default();
            let status = status_map.get(&file).cloned().flatten();
            result.push(SnapshotFileDiff {
                file,
                patch,
                additions,
                deletions,
                status,
            });
        }

        Ok(result)
    }

    pub async fn restore(&self, snapshot_hash: &str) -> Result<()> {
        let _guard = self.lock.lock().await;
        let (code, stdout, stderr) = git_run(
            &self.gitdir,
            &self.worktree,
            &["checkout", snapshot_hash, "--", "."],
        )
        .await?;
        if code != 0 {
            anyhow::bail!("git checkout failed ({}): {} {}", code, stdout, stderr);
        }
        Ok(())
    }

    pub async fn revert(&self, snapshot_hash: &str, files: &[String]) -> Result<()> {
        let _guard = self.lock.lock().await;
        let mut args = vec!["checkout", snapshot_hash, "--"];
        let file_refs: Vec<&str> = files.iter().map(|f| f.as_str()).collect();
        args.extend_from_slice(&file_refs);

        let (code, stdout, stderr) = git_run(&self.gitdir, &self.worktree, &args).await?;
        if code != 0 {
            anyhow::bail!("git checkout failed ({}): {} {}", code, stdout, stderr);
        }
        Ok(())
    }

    pub async fn cleanup(&self, keep_last_n: usize) -> Result<()> {
        let _guard = self.lock.lock().await;

        let (code, stdout, stderr) = git_run(
            &self.gitdir,
            &self.worktree,
            &["gc", "--prune=now"],
        )
        .await?;
        if code != 0 {
            anyhow::bail!("git gc failed ({}): {} {}", code, stdout, stderr);
        }

        let (code, stdout, stderr) = git_run(
            &self.gitdir,
            &self.worktree,
            &[
                "log",
                "--format=%H",
                &format!("-n{}", keep_last_n + 1),
            ],
        )
        .await?;
        if code != 0 {
            anyhow::bail!("git log failed ({}): {} {}", code, stdout, stderr);
        }

        let hashes: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
        if hashes.len() > keep_last_n {
            if let Some(&oldest) = hashes.last() {
                let (code, stdout, stderr) = git_run(
                    &self.gitdir,
                    &self.worktree,
                    &["update-ref", "-d", "HEAD", oldest],
                )
                .await?;
                if code != 0 {
                    tracing::warn!("update-ref -d failed: {} {}", stdout, stderr);
                }
            }
        }

        Ok(())
    }

    pub async fn list(&self, limit: usize) -> Result<Vec<(String, String)>> {
        let _guard = self.lock.lock().await;
        let n = format!("-n{}", limit);
        let (code, stdout, stderr) = git_run(
            &self.gitdir,
            &self.worktree,
            &["log", "--oneline", &n],
        )
        .await?;
        if code != 0 {
            anyhow::bail!("git log failed ({}): {} {}", code, stdout, stderr);
        }
        let mut result = Vec::new();
        for line in stdout.lines() {
            if line.is_empty() {
                continue;
            }
            if let Some((hash, msg)) = line.split_once(' ') {
                result.push((hash.to_string(), msg.to_string()));
            }
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_worktree() -> tempfile::TempDir {
        tempfile::tempdir().expect("failed to create tempdir")
    }

    #[tokio::test]
    async fn test_for_worktree_path() {
        let td = test_worktree();
        let sm = SnapshotManager::for_worktree(td.path());
        let hash = worktree_hash(td.path());
        assert!(
            sm.gitdir.to_string_lossy().contains(&hash),
            "gitdir should contain worktree hash"
        );
        assert!(
            sm.gitdir.to_string_lossy().contains(".agent/snapshots"),
            "gitdir should be under .agent/snapshots"
        );
        assert_eq!(sm.worktree, td.path());
    }

    #[tokio::test]
    async fn test_init_and_track() {
        let td = test_worktree();
        let sm = SnapshotManager::for_worktree(td.path());
        sm.init().await.expect("init failed");

        let hash = sm.track().await.expect("track failed");
        assert!(!hash.is_empty(), "commit hash should not be empty");
        assert_eq!(hash.len(), 40, "commit hash should be 40 hex chars");
    }

    #[tokio::test]
    async fn test_list_snapshots() {
        let td = test_worktree();
        let sm = SnapshotManager::for_worktree(td.path());
        sm.init().await.expect("init failed");

        sm.track().await.expect("track 1 failed");

        tokio::fs::write(td.path().join("a.txt"), "hello")
            .await
            .unwrap();
        sm.track().await.expect("track 2 failed");

        let list = sm.list(10).await.expect("list failed");
        assert_eq!(list.len(), 2, "expected 2 snapshots");
    }

    #[tokio::test]
    async fn test_diff() {
        let td = test_worktree();
        let sm = SnapshotManager::for_worktree(td.path());
        sm.init().await.expect("init failed");

        let h1 = sm.track().await.expect("track 1 failed");

        tokio::fs::write(td.path().join("hello.txt"), "world")
            .await
            .unwrap();
        sm.track().await.expect("track 2 failed");

        let changed = sm.patch(&h1).await.expect("patch failed");
        assert!(
            changed.iter().any(|f| f.contains("hello.txt")),
            "hello.txt should appear in diff"
        );
    }

    #[tokio::test]
    async fn test_restore() {
        let td = test_worktree();
        let sm = SnapshotManager::for_worktree(td.path());
        sm.init().await.expect("init failed");

        tokio::fs::write(td.path().join("restore.txt"), "v1")
            .await
            .unwrap();
        let h1 = sm.track().await.expect("track 1 failed");

        tokio::fs::write(td.path().join("restore.txt"), "v2")
            .await
            .unwrap();
        sm.track().await.expect("track 2 failed");

        sm.restore(&h1).await.expect("restore failed");

        let content = tokio::fs::read_to_string(td.path().join("restore.txt"))
            .await
            .unwrap();
        assert_eq!(content, "v1", "file should be restored to v1");
    }
}
