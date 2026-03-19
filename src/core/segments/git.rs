use super::{Segment, SegmentData};
use crate::config::{InputData, SegmentId};
use std::collections::HashMap;
use std::process::Command;

#[derive(Debug)]
pub struct GitFileStats {
    pub modified: u32,
    pub added: u32,
    pub deleted: u32,
    pub untracked: u32,
}

#[derive(Debug)]
pub struct GitInfo {
    pub branch: String,
    pub status: GitStatus,
    pub ahead: u32,
    pub behind: u32,
    pub sha: Option<String>,
    pub file_stats: Option<GitFileStats>,
}

#[derive(Debug, PartialEq)]
pub enum GitStatus {
    Clean,
    Dirty,
    Conflicts,
}

pub struct GitSegment {
    show_sha: bool,
    show_file_stats: bool,
}

impl Default for GitSegment {
    fn default() -> Self {
        Self::new()
    }
}

impl GitSegment {
    pub fn new() -> Self {
        Self { show_sha: false, show_file_stats: false }
    }

    pub fn with_sha(mut self, show_sha: bool) -> Self {
        self.show_sha = show_sha;
        self
    }

    pub fn with_file_stats(mut self, show_file_stats: bool) -> Self {
        self.show_file_stats = show_file_stats;
        self
    }

    fn get_git_info(&self, working_dir: &str) -> Option<GitInfo> {
        if !self.is_git_repository(working_dir) {
            return None;
        }

        let branch = self
            .get_branch(working_dir)
            .unwrap_or_else(|| "detached".to_string());
        let (status, file_stats_opt) = self.get_status_and_stats(working_dir);
        let (ahead, behind) = self.get_ahead_behind(working_dir);
        let sha = if self.show_sha {
            self.get_sha(working_dir)
        } else {
            None
        };

        let file_stats = if self.show_file_stats { file_stats_opt } else { None };

        Some(GitInfo {
            branch,
            status,
            ahead,
            behind,
            sha,
            file_stats,
        })
    }

    fn is_git_repository(&self, working_dir: &str) -> bool {
        Command::new("git")
            .args(["--no-optional-locks", "rev-parse", "--git-dir"])
            .current_dir(working_dir)
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    fn get_branch(&self, working_dir: &str) -> Option<String> {
        if let Ok(output) = Command::new("git")
            .args(["--no-optional-locks", "branch", "--show-current"])
            .current_dir(working_dir)
            .output()
        {
            if output.status.success() {
                let branch = String::from_utf8(output.stdout).ok()?.trim().to_string();
                if !branch.is_empty() {
                    return Some(branch);
                }
            }
        }

        if let Ok(output) = Command::new("git")
            .args(["--no-optional-locks", "symbolic-ref", "--short", "HEAD"])
            .current_dir(working_dir)
            .output()
        {
            if output.status.success() {
                let branch = String::from_utf8(output.stdout).ok()?.trim().to_string();
                if !branch.is_empty() {
                    return Some(branch);
                }
            }
        }

        None
    }

    fn get_status_and_stats(&self, working_dir: &str) -> (GitStatus, Option<GitFileStats>) {
        let output = Command::new("git")
            .args(["--no-optional-locks", "status", "--porcelain"])
            .current_dir(working_dir)
            .output();

        match output {
            Ok(output) if output.status.success() => {
                let status_text = String::from_utf8(output.stdout).unwrap_or_default();

                if status_text.trim().is_empty() {
                    return (GitStatus::Clean, Some(GitFileStats { modified: 0, added: 0, deleted: 0, untracked: 0 }));
                }

                let git_status = if status_text.contains("UU")
                    || status_text.contains("AA")
                    || status_text.contains("DD")
                {
                    GitStatus::Conflicts
                } else {
                    GitStatus::Dirty
                };

                let mut modified = 0u32;
                let mut added = 0u32;
                let mut deleted = 0u32;
                let mut untracked = 0u32;

                for line in status_text.lines() {
                    if line.len() < 2 {
                        continue;
                    }
                    let xy: &str = &line[..2];
                    match xy {
                        "??" => untracked += 1,
                        " D" | "D " | "DD" => deleted += 1,
                        " A" | "A " | "AA" | "AM" => added += 1,
                        " M" | "M " | "MM" | "RM" | "CM" => modified += 1,
                        _ => {
                            // Treat any other non-clean status as modified
                            if !xy.trim().is_empty() {
                                modified += 1;
                            }
                        }
                    }
                }

                let stats = GitFileStats { modified, added, deleted, untracked };
                (git_status, Some(stats))
            }
            _ => (GitStatus::Clean, None),
        }
    }

    fn get_ahead_behind(&self, working_dir: &str) -> (u32, u32) {
        let ahead = self.get_commit_count(working_dir, "@{u}..HEAD");
        let behind = self.get_commit_count(working_dir, "HEAD..@{u}");
        (ahead, behind)
    }

    fn get_commit_count(&self, working_dir: &str, range: &str) -> u32 {
        let output = Command::new("git")
            .args(["--no-optional-locks", "rev-list", "--count", range])
            .current_dir(working_dir)
            .output();

        match output {
            Ok(output) if output.status.success() => String::from_utf8(output.stdout)
                .ok()
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(0),
            _ => 0,
        }
    }

    fn get_sha(&self, working_dir: &str) -> Option<String> {
        let output = Command::new("git")
            .args(["--no-optional-locks", "rev-parse", "--short=7", "HEAD"])
            .current_dir(working_dir)
            .output()
            .ok()?;

        if output.status.success() {
            let sha = String::from_utf8(output.stdout).ok()?.trim().to_string();
            if sha.is_empty() {
                None
            } else {
                Some(sha)
            }
        } else {
            None
        }
    }
}

impl Segment for GitSegment {
    fn collect(&self, input: &InputData) -> Option<SegmentData> {
        let git_info = self.get_git_info(&input.workspace.current_dir)?;

        let mut metadata = HashMap::new();
        metadata.insert("branch".to_string(), git_info.branch.clone());
        metadata.insert("status".to_string(), format!("{:?}", git_info.status));
        metadata.insert("ahead".to_string(), git_info.ahead.to_string());
        metadata.insert("behind".to_string(), git_info.behind.to_string());

        if let Some(ref sha) = git_info.sha {
            metadata.insert("sha".to_string(), sha.clone());
        }

        let primary = git_info.branch;
        let mut status_parts = Vec::new();

        match git_info.status {
            GitStatus::Clean => status_parts.push("✓".to_string()),
            GitStatus::Dirty => status_parts.push("●".to_string()),
            GitStatus::Conflicts => status_parts.push("⚠".to_string()),
        }

        if git_info.ahead > 0 {
            status_parts.push(format!("↑{}", git_info.ahead));
        }
        if git_info.behind > 0 {
            status_parts.push(format!("↓{}", git_info.behind));
        }

        if let Some(ref sha) = git_info.sha {
            status_parts.push(sha.clone());
        }

        // Append file stats if enabled
        if let Some(ref stats) = git_info.file_stats {
            if stats.modified > 0 {
                status_parts.push(format!("!{}", stats.modified));
            }
            if stats.added > 0 {
                status_parts.push(format!("+{}", stats.added));
            }
            if stats.deleted > 0 {
                status_parts.push(format!("✘{}", stats.deleted));
            }
            if stats.untracked > 0 {
                status_parts.push(format!("?{}", stats.untracked));
            }
        }

        Some(SegmentData {
            primary,
            secondary: status_parts.join(" "),
            metadata,
        })
    }

    fn id(&self) -> SegmentId {
        SegmentId::Git
    }
}
