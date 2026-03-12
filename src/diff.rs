use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, TimeZone, Utc};
use git2::{BranchType, Delta, Diff, DiffOptions, Oid, Repository, Sort};
use ratatui::style::Style;

use crate::syntax::{StyledSegments, highlight_file};

pub const DEFAULT_COMMIT_LIMIT: usize = 10;
pub const WORKING_TREE_SELECTION_ID: &str = "__copanion_working_tree__";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitInfo {
    pub id: String,
    pub short_id: String,
    pub branch_name: Option<String>,
    pub summary: String,
    pub body: Option<String>,
    pub author: String,
    pub time: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineOrigin {
    Context,
    Addition,
    Deletion,
}

#[derive(Debug, Clone)]
pub struct DiffLine {
    pub origin: LineOrigin,
    pub content: String,
    pub old_lineno: Option<usize>,
    pub new_lineno: Option<usize>,
    pub segments: StyledSegments,
}

#[derive(Debug, Clone)]
pub struct DiffHunk {
    pub header: String,
    pub lines: Vec<DiffLine>,
    pub old_start: usize,
    pub old_count: usize,
    pub new_start: usize,
    pub new_count: usize,
}

#[derive(Debug, Clone)]
pub struct DiffFile {
    pub old_path: Option<String>,
    pub new_path: Option<String>,
    pub status: FileStatus,
    pub hunks: Vec<DiffHunk>,
    pub is_binary: bool,
    pub is_too_large: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffSelection {
    WorkingTree,
    CommitRange(Vec<String>),
    WorkingTreeAndCommits(Vec<String>),
}

pub struct GitDiffLoader {
    repo: Repository,
    root: PathBuf,
}

struct HighlightSequences {
    old_lines: Vec<String>,
    new_lines: Vec<String>,
    old_line_indices: Vec<Option<usize>>,
    new_line_indices: Vec<Option<usize>>,
}

impl CommitInfo {
    pub fn working_tree_entry() -> Self {
        Self {
            id: WORKING_TREE_SELECTION_ID.to_string(),
            short_id: "WORKTREE".to_string(),
            branch_name: None,
            summary: "Uncommitted changes".to_string(),
            body: None,
            author: String::new(),
            time: Utc::now(),
        }
    }

    pub fn is_working_tree(&self) -> bool {
        self.id == WORKING_TREE_SELECTION_ID
    }
}

impl FileStatus {
    pub const fn as_char(self) -> char {
        match self {
            Self::Added => 'A',
            Self::Modified => 'M',
            Self::Deleted => 'D',
            Self::Renamed => 'R',
            Self::Copied => 'C',
        }
    }
}

impl DiffFile {
    pub fn display_path(&self) -> &str {
        self.new_path
            .as_deref()
            .or(self.old_path.as_deref())
            .expect("diff file must have at least one path")
    }
}

impl GitDiffLoader {
    pub fn discover(start: &Path) -> Result<Self> {
        let repo = Repository::discover(start).with_context(|| {
            format!(
                "failed to discover a git repository from {}",
                start.display()
            )
        })?;
        let root = repo
            .workdir()
            .context("copanion diff mode requires a non-bare git working tree")?
            .to_path_buf();
        Ok(Self { repo, root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn selection_options(&self, limit: usize) -> Result<Vec<CommitInfo>> {
        let has_working_tree = self.has_working_tree_changes()?;
        let mut options = self.recent_commits(limit)?;
        if has_working_tree {
            options.insert(0, CommitInfo::working_tree_entry());
        }
        if options.is_empty() {
            bail!("no recent commits or uncommitted changes to inspect")
        }
        Ok(options)
    }

    pub fn recent_commits(&self, limit: usize) -> Result<Vec<CommitInfo>> {
        let mut revwalk = match self.repo.revwalk() {
            Ok(revwalk) => revwalk,
            Err(_) => return Ok(Vec::new()),
        };
        if revwalk.push_head().is_err() {
            return Ok(Vec::new());
        }
        revwalk.set_sorting(Sort::TOPOLOGICAL | Sort::TIME)?;
        let branch_tip_names = self.branch_tip_names();

        let mut commits = Vec::new();
        for oid in revwalk.take(limit) {
            let oid = oid?;
            let commit = self.repo.find_commit(oid)?;
            let message = commit.message().unwrap_or("(no message)");
            let (summary, body) = parse_commit_message(message);
            commits.push(CommitInfo {
                id: oid.to_string(),
                short_id: short_id(oid),
                branch_name: branch_tip_names
                    .get(&oid)
                    .and_then(|names| names.first().cloned()),
                summary,
                body,
                author: commit.author().name().unwrap_or("Unknown").to_string(),
                time: Utc
                    .timestamp_opt(commit.time().seconds(), 0)
                    .single()
                    .unwrap_or_else(Utc::now),
            });
        }

        Ok(commits)
    }

    pub fn diff_for_selection(&self, selection: &DiffSelection) -> Result<Vec<DiffFile>> {
        match selection {
            DiffSelection::WorkingTree => self.get_working_tree_diff(),
            DiffSelection::CommitRange(commit_ids) => self.get_commit_range_diff(commit_ids),
            DiffSelection::WorkingTreeAndCommits(commit_ids) => {
                self.get_working_tree_with_commits_diff(commit_ids)
            }
        }
    }

    pub fn fetch_context_lines(
        &self,
        file: &DiffFile,
        start_line: usize,
        end_line: usize,
    ) -> Result<Vec<DiffLine>> {
        if start_line == 0 || start_line > end_line {
            return Ok(Vec::new());
        }

        let path = file
            .new_path
            .as_deref()
            .or(file.old_path.as_deref())
            .context("diff file is missing a path")?;

        let content = match file.status {
            FileStatus::Deleted => self.fetch_blob_content(Path::new(
                file.old_path
                    .as_deref()
                    .context("deleted diff file is missing its original path")?,
            ))?,
            _ => fs::read_to_string(self.root.join(path))
                .with_context(|| format!("failed to read {}", self.root.join(path).display()))?,
        };

        let lines = content.lines().map(ToString::to_string).collect::<Vec<_>>();
        let highlighted = highlight_file(path, &lines);
        let mut result = Vec::new();

        for line_no in start_line..=end_line {
            let index = line_no.saturating_sub(1);
            if let Some(content) = lines.get(index) {
                let segments = highlighted
                    .get(index)
                    .cloned()
                    .unwrap_or_else(|| default_segments(content));
                result.push(DiffLine {
                    origin: LineOrigin::Context,
                    content: content.clone(),
                    old_lineno: Some(line_no),
                    new_lineno: if file.status == FileStatus::Deleted {
                        None
                    } else {
                        Some(line_no)
                    },
                    segments,
                });
            }
        }

        Ok(result)
    }

    fn has_working_tree_changes(&self) -> Result<bool> {
        match self.get_working_tree_diff() {
            Ok(diff_files) => Ok(!diff_files.is_empty()),
            Err(_) => Ok(false),
        }
    }

    fn branch_tip_names(&self) -> HashMap<Oid, Vec<String>> {
        let mut names_by_tip = HashMap::<Oid, Vec<String>>::new();

        let Ok(branches) = self.repo.branches(Some(BranchType::Local)) else {
            return names_by_tip;
        };

        for (branch, _) in branches.flatten() {
            let Some(target) = branch.get().target() else {
                continue;
            };
            let Ok(Some(name)) = branch.name() else {
                continue;
            };
            names_by_tip
                .entry(target)
                .or_default()
                .push(name.to_string());
        }

        for names in names_by_tip.values_mut() {
            names.sort_unstable();
        }

        names_by_tip
    }

    fn get_working_tree_diff(&self) -> Result<Vec<DiffFile>> {
        let mut opts = DiffOptions::new();
        opts.include_untracked(true);
        opts.show_untracked_content(true);
        opts.recurse_untracked_dirs(true);

        let head_tree = head_tree(&self.repo)?;
        let diff = self
            .repo
            .diff_tree_to_workdir_with_index(head_tree.as_ref(), Some(&mut opts))?;

        parse_diff(&diff)
    }

    fn get_commit_range_diff(&self, commit_ids: &[String]) -> Result<Vec<DiffFile>> {
        if commit_ids.is_empty() {
            bail!("no commits selected for diff mode")
        }

        let oldest = self.repo.find_commit(Oid::from_str(&commit_ids[0])?)?;
        let newest = self.repo.find_commit(Oid::from_str(
            commit_ids.last().expect("commit list is non-empty"),
        )?)?;

        let old_tree = if oldest.parent_count() > 0 {
            Some(oldest.parent(0)?.tree()?)
        } else {
            None
        };
        let new_tree = newest.tree()?;
        let diff = self
            .repo
            .diff_tree_to_tree(old_tree.as_ref(), Some(&new_tree), None)?;

        parse_diff(&diff)
    }

    fn get_working_tree_with_commits_diff(&self, commit_ids: &[String]) -> Result<Vec<DiffFile>> {
        if commit_ids.is_empty() {
            bail!("no commits selected for combined diff mode")
        }

        let oldest = self.repo.find_commit(Oid::from_str(&commit_ids[0])?)?;
        let old_tree = if oldest.parent_count() > 0 {
            Some(oldest.parent(0)?.tree()?)
        } else {
            None
        };

        let mut opts = DiffOptions::new();
        opts.include_untracked(true);
        opts.show_untracked_content(true);
        opts.recurse_untracked_dirs(true);

        let diff = self
            .repo
            .diff_tree_to_workdir_with_index(old_tree.as_ref(), Some(&mut opts))?;

        parse_diff(&diff)
    }

    fn fetch_blob_content(&self, file_path: &Path) -> Result<String> {
        let head = self.repo.head()?.peel_to_tree()?;
        let entry = head.get_path(file_path)?;
        let blob = self.repo.find_blob(entry.id())?;
        let content = std::str::from_utf8(blob.content())
            .context("failed to decode blob content as UTF-8 for diff context expansion")?;
        Ok(content.to_string())
    }
}

fn head_tree(repo: &Repository) -> Result<Option<git2::Tree<'_>>> {
    match repo.head() {
        Ok(head) => Ok(Some(head.peel_to_tree()?)),
        Err(err) if err.code() == git2::ErrorCode::UnbornBranch => Ok(None),
        Err(err) => Err(err.into()),
    }
}

fn parse_commit_message(message: &str) -> (String, Option<String>) {
    let mut lines = message.lines();
    let summary = lines.next().unwrap_or("(no message)").to_string();
    let body_text = lines
        .skip_while(|line| line.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let body = (!body_text.trim().is_empty()).then_some(body_text);
    (summary, body)
}

fn short_id(oid: Oid) -> String {
    let id = oid.to_string();
    id[..7.min(id.len())].to_string()
}

pub fn calculate_gap(previous_hunk: Option<&DiffHunk>, current_hunk: &DiffHunk) -> usize {
    match previous_hunk {
        None => current_hunk.new_start.saturating_sub(1),
        Some(previous) => {
            let previous_end = previous.new_start.saturating_add(previous.new_count);
            current_hunk.new_start.saturating_sub(previous_end)
        }
    }
}

fn parse_diff(diff: &Diff<'_>) -> Result<Vec<DiffFile>> {
    let mut files = Vec::new();
    const MAX_UNTRACKED_FILE_SIZE: u64 = 10 * 1_024 * 1_024;

    for (delta_idx, delta) in diff.deltas().enumerate() {
        let status = match delta.status() {
            Delta::Added | Delta::Untracked => FileStatus::Added,
            Delta::Deleted => FileStatus::Deleted,
            Delta::Modified => FileStatus::Modified,
            Delta::Renamed => FileStatus::Renamed,
            Delta::Copied => FileStatus::Copied,
            _ => FileStatus::Modified,
        };

        let old_path = delta.old_file().path().map(normalize_path);
        let new_path = delta.new_file().path().map(normalize_path);
        let is_binary = delta.old_file().is_binary() || delta.new_file().is_binary();
        let is_too_large =
            delta.status() == Delta::Untracked && delta.new_file().size() > MAX_UNTRACKED_FILE_SIZE;

        let display_path = new_path.as_deref().or(old_path.as_deref());
        let hunks = if is_binary || is_too_large {
            Vec::new()
        } else {
            parse_hunks(diff, delta_idx, display_path)?
        };

        files.push(DiffFile {
            old_path,
            new_path,
            status,
            hunks,
            is_binary,
            is_too_large,
        });
    }

    if files.is_empty() {
        bail!("no changes found for the selected diff")
    }

    Ok(files)
}

fn parse_hunks(diff: &Diff<'_>, delta_idx: usize, path: Option<&str>) -> Result<Vec<DiffHunk>> {
    let mut hunks = Vec::new();
    let Some(patch) = git2::Patch::from_diff(diff, delta_idx)? else {
        return Ok(hunks);
    };

    for hunk_idx in 0..patch.num_hunks() {
        let (hunk, _) = patch.hunk(hunk_idx)?;
        let mut line_contents = Vec::new();
        let mut line_origins = Vec::new();
        let mut line_numbers = Vec::new();

        for line_idx in 0..patch.num_lines_in_hunk(hunk_idx)? {
            let line = patch.line_in_hunk(hunk_idx, line_idx)?;
            let origin = match line.origin() {
                '+' => LineOrigin::Addition,
                '-' => LineOrigin::Deletion,
                _ => LineOrigin::Context,
            };
            let content = String::from_utf8_lossy(line.content())
                .trim_end_matches('\n')
                .trim_end_matches('\r')
                .replace('\t', "    ")
                .to_string();
            line_contents.push(content);
            line_origins.push(origin);
            line_numbers.push((
                line.old_lineno().map(|line| line as usize),
                line.new_lineno().map(|line| line as usize),
            ));
        }

        let highlights = build_highlights(path, &line_contents, &line_origins);
        let lines = line_contents
            .into_iter()
            .zip(line_origins.into_iter())
            .zip(line_numbers.into_iter())
            .zip(highlights.into_iter())
            .map(
                |(((content, origin), (old_lineno, new_lineno)), segments)| DiffLine {
                    origin,
                    content,
                    old_lineno,
                    new_lineno,
                    segments,
                },
            )
            .collect();

        hunks.push(DiffHunk {
            header: String::from_utf8_lossy(hunk.header()).trim().to_string(),
            lines,
            old_start: hunk.old_start() as usize,
            old_count: hunk.old_lines() as usize,
            new_start: hunk.new_start() as usize,
            new_count: hunk.new_lines() as usize,
        });
    }

    Ok(hunks)
}

fn build_highlights(
    path: Option<&str>,
    lines: &[String],
    origins: &[LineOrigin],
) -> Vec<StyledSegments> {
    let Some(path) = path else {
        return lines.iter().map(|line| default_segments(line)).collect();
    };

    let sequences = split_diff_lines_for_highlighting(lines, origins);
    let old_highlighted = if sequences.old_lines.is_empty() {
        Vec::new()
    } else {
        highlight_file(path, &sequences.old_lines)
    };
    let new_highlighted = if sequences.new_lines.is_empty() {
        Vec::new()
    } else {
        highlight_file(path, &sequences.new_lines)
    };

    lines
        .iter()
        .enumerate()
        .map(|(index, line)| match origins[index] {
            LineOrigin::Deletion => sequences.old_line_indices[index]
                .and_then(|line_idx| old_highlighted.get(line_idx).cloned())
                .unwrap_or_else(|| default_segments(line)),
            LineOrigin::Context | LineOrigin::Addition => sequences.new_line_indices[index]
                .and_then(|line_idx| new_highlighted.get(line_idx).cloned())
                .unwrap_or_else(|| default_segments(line)),
        })
        .collect()
}

fn split_diff_lines_for_highlighting(
    lines: &[String],
    origins: &[LineOrigin],
) -> HighlightSequences {
    let mut old_lines = Vec::new();
    let mut new_lines = Vec::new();
    let mut old_line_indices = Vec::with_capacity(lines.len());
    let mut new_line_indices = Vec::with_capacity(lines.len());

    for (line, origin) in lines.iter().zip(origins.iter().copied()) {
        match origin {
            LineOrigin::Context => {
                let old_index = old_lines.len();
                let new_index = new_lines.len();
                old_lines.push(line.clone());
                new_lines.push(line.clone());
                old_line_indices.push(Some(old_index));
                new_line_indices.push(Some(new_index));
            }
            LineOrigin::Deletion => {
                let old_index = old_lines.len();
                old_lines.push(line.clone());
                old_line_indices.push(Some(old_index));
                new_line_indices.push(None);
            }
            LineOrigin::Addition => {
                let new_index = new_lines.len();
                new_lines.push(line.clone());
                old_line_indices.push(None);
                new_line_indices.push(Some(new_index));
            }
        }
    }

    HighlightSequences {
        old_lines,
        new_lines,
        old_line_indices,
        new_line_indices,
    }
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn default_segments(line: &str) -> StyledSegments {
    vec![(Style::default(), line.to_string())]
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use git2::{Repository, Signature};
    use tempfile::tempdir;

    use super::{
        CommitInfo, DEFAULT_COMMIT_LIMIT, DiffSelection, GitDiffLoader, WORKING_TREE_SELECTION_ID,
    };

    fn init_repo() -> (tempfile::TempDir, Repository) {
        let temp = tempdir().unwrap();
        let repo = Repository::init(temp.path()).unwrap();
        (temp, repo)
    }

    fn commit_file(repo: &Repository, path: &str, contents: &str, message: &str) -> String {
        let workdir = repo.workdir().unwrap();
        if let Some(parent) = workdir.join(path).parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(workdir.join(path), contents).unwrap();

        let mut index = repo.index().unwrap();
        index.add_path(Path::new(path)).unwrap();
        index.write().unwrap();

        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let sig = Signature::now("Test User", "test@example.com").unwrap();
        let parent = repo.head().ok().and_then(|head| head.peel_to_commit().ok());
        let parents = parent.iter().collect::<Vec<_>>();

        let commit_id = repo
            .commit(Some("HEAD"), &sig, &sig, message, &tree, &parents)
            .unwrap();

        commit_id.to_string()
    }

    #[test]
    fn selection_options_include_working_tree_row() {
        let (temp, repo) = init_repo();
        commit_file(&repo, "src/main.rs", "fn main() {}\n", "initial");
        fs::write(
            temp.path().join("src/main.rs"),
            "fn main() {\n    println!(\"hi\");\n}\n",
        )
        .unwrap();

        let loader = GitDiffLoader::discover(temp.path()).unwrap();
        let options = loader.selection_options(DEFAULT_COMMIT_LIMIT).unwrap();

        assert_eq!(
            options.first().map(|entry| entry.id.as_str()),
            Some(WORKING_TREE_SELECTION_ID)
        );
        assert!(options.len() >= 2);
    }

    #[test]
    fn commit_range_diff_reads_oldest_to_newest_selection() {
        let (temp, repo) = init_repo();
        let first = commit_file(&repo, "src/main.rs", "fn main() {}\n", "initial");
        let second = commit_file(
            &repo,
            "src/main.rs",
            "fn main() {\n    println!(\"two\");\n}\n",
            "add print",
        );

        let loader = GitDiffLoader::discover(temp.path()).unwrap();
        let diff_files = loader
            .diff_for_selection(&DiffSelection::CommitRange(vec![first, second]))
            .unwrap();

        assert_eq!(diff_files.len(), 1);
        assert_eq!(diff_files[0].display_path(), "src/main.rs");
        assert!(
            diff_files[0]
                .hunks
                .iter()
                .flat_map(|hunk| hunk.lines.iter())
                .any(|line| line.content.contains("println!(\"two\")"))
        );
    }

    #[test]
    fn combined_selection_includes_working_tree_changes_after_commits() {
        let (temp, repo) = init_repo();
        commit_file(&repo, "src/main.rs", "fn main() {}\n", "initial");
        let second = commit_file(
            &repo,
            "src/main.rs",
            "fn main() {\n    println!(\"two\");\n}\n",
            "add print",
        );
        fs::write(
            temp.path().join("src/main.rs"),
            "fn main() {\n    println!(\"three\");\n}\n",
        )
        .unwrap();

        let loader = GitDiffLoader::discover(temp.path()).unwrap();
        let diff_files = loader
            .diff_for_selection(&DiffSelection::WorkingTreeAndCommits(vec![second]))
            .unwrap();

        assert!(
            diff_files[0]
                .hunks
                .iter()
                .flat_map(|hunk| hunk.lines.iter())
                .any(|line| line.content.contains("println!(\"three\")"))
        );
    }

    #[test]
    fn working_tree_entry_uses_stable_sentinel() {
        let entry = CommitInfo::working_tree_entry();
        assert_eq!(entry.id, WORKING_TREE_SELECTION_ID);
        assert!(entry.is_working_tree());
    }
}
