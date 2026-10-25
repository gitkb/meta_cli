//! `--include` / `--exclude`: which repos a command runs in.

use anyhow::{bail, Result};
use meta_core::config::{MetaTreeNode, ProjectInfo};
use std::path::{Component, Path};

/// (`.meta` name, path from the meta root) for `projects`, and for every
/// project nested under `tree` (the result of walking the same meta root).
pub fn repo_names(projects: &[ProjectInfo], tree: &[MetaTreeNode]) -> Vec<(String, String)> {
    fn walk(nodes: &[MetaTreeNode], prefix: &str, out: &mut Vec<(String, String)>) {
        for node in nodes {
            let path = normalize(&format!("{prefix}/{}", node.info.path));
            out.push((node.info.name.clone(), path.clone()));
            walk(&node.children, &path, out);
        }
    }
    let mut names: Vec<(String, String)> = projects
        .iter()
        .map(|p| (p.name.clone(), normalize(&p.path)))
        .collect();
    walk(tree, "", &mut names);
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
    let paths: Vec<String> = dirs.iter().map(|dir| repo_path(dir, root)).collect();
    let named = |flag: &str, value: &str| -> Result<Vec<&str>> {
        let wanted = path_value(value);
        let hits: Vec<&str> = paths
            .iter()
            .filter(|path| {
                wanted.as_deref() == Some(path.as_str())
                    || names.iter().any(|(name, p)| {
                        (name == value || wanted.as_deref() == Some(name.as_str())) && p == *path
                    })
            })
            .map(String::as_str)
            .collect();
        if hits.is_empty() {
            bail!(
                "{flag} '{value}' names no repo here. Use a repo's name from .meta or its path \
                 from the workspace root (`.` for the root), as `meta project list` shows it."
            );
        }
        Ok(hits)
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
/// as given when it is not under `root`. A relative `dir` is taken from the
/// current directory.
fn repo_path(dir: &str, root: &Path) -> String {
    let absolute = std::env::current_dir()
        .map(|cwd| cwd.join(dir))
        .unwrap_or_else(|_| dir.into());
    match absolute.strip_prefix(root) {
        Ok(rel) => match normalize(&rel.to_string_lossy()) {
            rel if rel.is_empty() => ".".to_string(),
            rel => rel,
        },
        Err(_) => dir.to_string(),
    }
}

/// A filter value as the path it names from the root: `.` for the root,
/// otherwise [`normalize`]d. None for a value that cannot be such a path (empty,
/// absolute, or with `..`).
fn path_value(value: &str) -> Option<String> {
    let escapes = Path::new(value)
        .components()
        .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir));
    match normalize(value) {
        _ if escapes || value.is_empty() => None,
        path if path.is_empty() => Some(".".to_string()),
        path => Some(path),
    }
}

/// `path` as `/`-separated names, without `.` parts or a trailing `/`; `.`
/// alone becomes empty.
fn normalize(path: &str) -> String {
    let parts: Vec<_> = Path::new(path)
        .components()
        .filter_map(|c| match c {
            Component::Normal(name) => Some(name.to_string_lossy()),
            _ => None,
        })
        .collect();
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn values_name_whole_repos_never_parts_of_paths() {
        let root = std::env::temp_dir().join("ws").join("a");
        let rels = ["", "api", "api-legacy", "libs/core"];
        let dirs: Vec<String> = rels
            .iter()
            .map(|rel| root.join(rel).display().to_string())
            .collect();
        let projects = [ProjectInfo {
            name: "core-lib".to_string(),
            path: "./libs/core/".to_string(),
            repo: None,
            tags: vec![],
            provides: vec![],
            depends_on: vec![],
            meta: false,
        }];
        let names = repo_names(&projects, &[]);
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
            assert!(err.contains("names no repo"), "{value}: {err}");
            let err = select(&[], &[value]).unwrap_err().to_string();
            assert!(err.contains("--exclude"), "{value}: {err}");
        }
        let err = select(&["api"], &["api"]).unwrap_err().to_string();
        assert!(err.contains("leave no repo"), "{err}");
    }
}
