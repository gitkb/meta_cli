//! `--include` / `--exclude`: which repos a command runs in.

use anyhow::{bail, Result};
use meta_core::config::{MetaTreeNode, ProjectInfo};
use std::path::{Component, Path};

/// (`.meta` name, path from `meta_dir`) for `projects`, and for every project
/// nested under `tree` (the result of walking the same meta root). Paths are
/// in the same form as [`select`] gives the repos it chooses among.
pub fn repo_names(
    meta_dir: &Path,
    projects: &[ProjectInfo],
    tree: &[MetaTreeNode],
) -> Vec<(String, String)> {
    fn walk(nodes: &[MetaTreeNode], dir: &Path, root: &Path, out: &mut Vec<(String, String)>) {
        for node in nodes {
            let path = dir.join(&node.info.path);
            out.push((node.info.name.clone(), repo_path(&path, root)));
            walk(&node.children, &path, root, out);
        }
    }
    let mut names: Vec<(String, String)> = projects
        .iter()
        .map(|p| (p.name.clone(), repo_path(&meta_dir.join(&p.path), meta_dir)))
        .collect();
    walk(tree, meta_dir, meta_dir, &mut names);
    names
}

/// The repos among `dirs` that `include` and `exclude` select, in order.
///
/// A value names a repo by its `.meta` name (from `names`, see
/// [`repo_names`]) or by its whole path from `root` (`.` for the root; a
/// leading `./` and a trailing `/` are ignored), never by a part of either.
/// A value that names none of `dirs` is an error, so a typo never runs a
/// command in a repo it was meant to leave out; so is a selection that leaves
/// no repo.
pub fn select(
    include: &[String],
    exclude: &[String],
    dirs: &[String],
    root: &Path,
    names: &[(String, String)],
) -> Result<Vec<String>> {
    if include.is_empty() && exclude.is_empty() {
        return Ok(dirs.to_vec());
    }
    let paths: Vec<String> = dirs
        .iter()
        .map(|dir| repo_path(Path::new(dir), root))
        .collect();
    let named = |flag: &str, value: &str| -> Result<Vec<&str>> {
        let wanted = path_value(value);
        let is_name = |name: &String| name == value || wanted.as_deref() == Some(name.as_str());
        let by_path: Vec<&str> = paths
            .iter()
            .filter(|path| wanted.as_deref() == Some(path.as_str()))
            .map(String::as_str)
            .collect();
        let by_name: Vec<&str> = paths
            .iter()
            .filter(|path| names.iter().any(|(name, p)| is_name(name) && p == *path))
            .map(String::as_str)
            .collect();
        if by_name.len() > 1 {
            bail!(
                "{flag} '{value}' is the .meta name of {}; name the repo by its path",
                by_name.join(", ")
            );
        }
        if !by_path.is_empty() && !by_name.is_empty() && by_path != by_name {
            bail!(
                "{flag} '{value}' is the path of {} and the .meta name of {}; name the repo by \
                 its path",
                by_path.join(", "),
                by_name.join(", ")
            );
        }
        if by_path.is_empty() && by_name.is_empty() {
            bail!(
                "{flag} '{value}' names none of the repos this command runs in. Use a repo's \
                 name from .meta or its path from the workspace root (`.` for the root), as \
                 `meta project list` shows it; in a worktree set, its alias. --tag and \
                 --recursive change which repos the command runs in."
            );
        }
        Ok(if by_path.is_empty() { by_name } else { by_path })
    };
    let mut included = Vec::new();
    for value in include {
        included.extend(named("--include", value)?);
    }
    let mut excluded = Vec::new();
    for value in exclude {
        excluded.extend(named("--exclude", value)?);
    }
    let selected: Vec<String> = dirs
        .iter()
        .zip(&paths)
        .filter(|(_, path)| include.is_empty() || included.contains(&path.as_str()))
        .filter(|(_, path)| !excluded.contains(&path.as_str()))
        .map(|(dir, _)| dir.clone())
        .collect();
    if selected.is_empty() {
        bail!("--include/--exclude leave no repo to run in");
    }
    Ok(selected)
}

/// `dir`'s path from `root`, `/`-separated and `.` for the root itself; `dir`
/// as given when it is not under `root`. Relative paths are taken from the
/// current directory.
fn repo_path(dir: &Path, root: &Path) -> String {
    let absolute = |path: &Path| {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    };
    match absolute(dir).strip_prefix(absolute(root)) {
        Ok(rel) => match normalize(&rel.to_string_lossy()) {
            rel if rel.is_empty() => ".".to_string(),
            rel => rel,
        },
        Err(_) => dir.display().to_string(),
    }
}

/// A filter value as the path it names from the root: `.` for the root,
/// otherwise [`normalize`]d. None for a value that cannot be such a path
/// (empty or absolute).
fn path_value(value: &str) -> Option<String> {
    let absolute = Path::new(value)
        .components()
        .any(|c| matches!(c, Component::RootDir | Component::Prefix(_)));
    match normalize(value) {
        _ if absolute || value.is_empty() => None,
        path if path.is_empty() => Some(".".to_string()),
        path => Some(path),
    }
}

/// `path` `/`-separated, without `.` parts or a trailing `/`; `.` alone
/// becomes empty. `..` parts stay: dropping them would give two different
/// directories the same name.
fn normalize(path: &str) -> String {
    let parts: Vec<_> = Path::new(path)
        .components()
        .filter(|c| !matches!(c, Component::CurDir))
        .map(|c| c.as_os_str().to_string_lossy())
        .collect();
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|s| s.to_string()).collect()
    }

    fn project(name: &str, path: &str) -> ProjectInfo {
        ProjectInfo {
            name: name.to_string(),
            path: path.to_string(),
            repo: None,
            tags: vec![],
            provides: vec![],
            depends_on: vec![],
            meta: false,
        }
    }

    #[test]
    fn values_name_whole_repos_never_parts_of_paths() {
        let root = std::env::temp_dir().join("ws").join("a");
        let rels = ["", "api", "api-legacy", "libs/core"];
        let dirs: Vec<String> = rels
            .iter()
            .map(|rel| root.join(rel).display().to_string())
            .collect();
        let names = repo_names(&root, &[project("core-lib", "./libs/core/")], &[]);
        let select = |include: &[&str], exclude: &[&str]| {
            select(&strings(include), &strings(exclude), &dirs, &root, &names)
        };
        let picked = |include: &[&str], exclude: &[&str]| -> Vec<String> {
            let dir = |rel: &str| root.join(rel).display().to_string();
            let chosen = select(include, exclude).unwrap();
            rels.iter()
                .filter(|rel| chosen.contains(&dir(rel)))
                .map(|rel| if rel.is_empty() { "." } else { rel }.to_string())
                .collect()
        };

        assert_eq!(picked(&["api"], &[]), ["api"]);
        assert_eq!(picked(&["./api/"], &[]), ["api"]);
        assert_eq!(picked(&["."], &[]), ["."]);
        assert_eq!(picked(&["libs/core"], &[]), ["libs/core"]);
        assert_eq!(picked(&["core-lib"], &[]), ["libs/core"]);
        assert_eq!(picked(&["./core-lib/"], &[]), ["libs/core"]);
        assert_eq!(picked(&[], &["api"]), [".", "api-legacy", "libs/core"]);
        assert_eq!(picked(&["api", "api-legacy"], &["api"]), ["api-legacy"]);
        assert_eq!(picked(&[], &[]), [".", "api", "api-legacy", "libs/core"]);
        // Parts of a name or of the absolute path name nothing
        let absolute = root.join("api").display().to_string();
        for value in ["a", "ap", "core", "ws", absolute.as_str(), ""] {
            let err = select(&[value], &[]).unwrap_err().to_string();
            assert!(err.contains("names none of the repos"), "{value}: {err}");
            let err = select(&[], &[value]).unwrap_err().to_string();
            assert!(err.contains("--exclude"), "{value}: {err}");
        }
        let err = select(&["api"], &["api"]).unwrap_err().to_string();
        assert!(err.contains("leave no repo"), "{err}");
    }

    #[test]
    fn parent_parts_keep_different_repos_apart() {
        let root = std::env::temp_dir().join("ws").join("a");
        let rels = ["", "..", "api", "libs/../api"];
        let dirs: Vec<String> = rels
            .iter()
            .map(|rel| root.join(rel).display().to_string())
            .collect();
        let names = repo_names(
            &root,
            &[project("parent", ".."), project("outer", "libs/../api")],
            &[],
        );
        for (value, rel) in [
            (".", ""),
            ("parent", ".."),
            ("api", "api"),
            ("outer", "libs/../api"),
        ] {
            let chosen = select(&strings(&[value]), &[], &dirs, &root, &names).unwrap();
            assert_eq!(chosen, [root.join(rel).display().to_string()], "{value}");
        }
    }

    #[test]
    fn names_from_the_tree_match_nested_and_top_level_repos() {
        let root = std::env::temp_dir().join("ws").join("a");
        let dirs: Vec<String> = ["", "parent", "parent/libs/core"]
            .iter()
            .map(|rel| root.join(rel).display().to_string())
            .collect();
        let node = |info: ProjectInfo, children| MetaTreeNode {
            info,
            is_meta: true,
            children,
        };
        let tree = [node(
            project("top", "parent"),
            vec![node(project("core-lib", "./libs/core"), vec![])],
        )];
        let names = repo_names(&root, &[], &tree);
        for (value, rel) in [("top", "parent"), ("core-lib", "parent/libs/core")] {
            let chosen = select(&strings(&[value]), &[], &dirs, &root, &names).unwrap();
            assert_eq!(chosen, [root.join(rel).display().to_string()], "{value}");
        }
    }

    #[test]
    fn absolute_paths_and_ambiguous_names() {
        let root = std::env::temp_dir().join("ws").join("a");
        let outside = std::env::temp_dir().join("elsewhere");
        let dirs: Vec<String> = [
            root.join("libs/core"),
            root.join("web"),
            root.join("frontend"),
            outside.clone(),
        ]
        .iter()
        .map(|dir| dir.display().to_string())
        .collect();
        let absolute = |dir: &std::path::Path| dir.display().to_string();
        let names = repo_names(
            &root,
            &[
                project("core-lib", &absolute(&root.join("libs/core"))),
                project("far", &absolute(&outside)),
                project("web", "frontend"),
            ],
            &[],
        );
        let select = |value: &str| select(&strings(&[value]), &[], &dirs, &root, &names);
        assert_eq!(select("core-lib").unwrap(), [dirs[0].clone()]);
        assert_eq!(select("far").unwrap(), [dirs[3].clone()]);
        // `web` is one repo's path and another's name
        let err = select("web").unwrap_err().to_string();
        assert!(
            err.contains("is the path of web and the .meta name of frontend"),
            "{err}"
        );
    }

    #[test]
    fn a_name_shared_by_two_repos_is_ambiguous_and_relative_roots_work() {
        let root = std::path::PathBuf::from("ws-relative");
        let dirs: Vec<String> = ["a/api", "b/api"]
            .iter()
            .map(|rel| root.join(rel).display().to_string())
            .collect();
        let names = repo_names(
            &root,
            &[project("api", "a/api"), project("api", "b/api")],
            &[],
        );
        let err = select(&strings(&["api"]), &[], &dirs, &root, &names)
            .unwrap_err()
            .to_string();
        assert!(err.contains("is the .meta name of a/api, b/api"), "{err}");
        // Paths still pick one, with a relative root as with an absolute one
        let chosen = select(&strings(&["b/api"]), &[], &dirs, &root, &names).unwrap();
        assert_eq!(chosen, [dirs[1].clone()]);
    }
}
