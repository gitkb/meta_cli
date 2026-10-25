//! Which repos an `--include` / `--exclude` value names.

use anyhow::{bail, Result};
use meta_core::config::{walk_meta_tree, MetaTreeNode};
use std::path::Path;

/// Every project under the meta repo at `meta_dir`, nested ones included, as
/// (`.meta` name, path from `meta_dir`).
pub fn repo_names(meta_dir: &Path) -> Vec<(String, String)> {
    fn walk(nodes: &[MetaTreeNode], prefix: &str, out: &mut Vec<(String, String)>) {
        for node in nodes {
            let path = if prefix.is_empty() {
                node.info.path.clone()
            } else {
                format!("{prefix}/{}", node.info.path)
            };
            out.push((node.info.name.clone(), path.clone()));
            walk(&node.children, &path, out);
        }
    }
    let mut names = Vec::new();
    walk(
        &walk_meta_tree(meta_dir, None).unwrap_or_default(),
        "",
        &mut names,
    );
    names
}

/// The paths from `root` of the repos among `dirs` that `values` name, for
/// loop_lib to match whole. A value names a repo by its `.meta` name (from
/// `names`, see [`repo_names`]) or by its path from `root` (`.` for the root),
/// never by a part of either. A value that names none of `dirs` is an error, so
/// a typo never runs a command in a repo it was meant to leave out.
pub fn resolve(
    flag: &str,
    values: &[String],
    dirs: &[String],
    root: &Path,
    names: &[(String, String)],
) -> Result<Vec<String>> {
    let mut paths: Vec<String> = Vec::new();
    for value in values {
        let mut found = false;
        for dir in dirs {
            let path = loop_lib::repo_path(dir, root).unwrap_or_else(|| dir.clone());
            let by_name = names.iter().any(|(name, p)| name == value && *p == path);
            if by_name || loop_lib::filter_matches(value, dir, Some(root)) {
                found = true;
                if !paths.contains(&path) {
                    paths.push(path);
                }
            }
        }
        if !found {
            bail!(
                "{flag} '{value}' names no repo here. Use a repo's name from .meta or its path \
                 from the workspace root (`.` for the root), as `meta project list` shows it."
            );
        }
    }
    Ok(paths)
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
        let dirs: Vec<String> = ["", "api", "api-legacy", "libs/core"]
            .iter()
            .map(|rel| root.join(rel).display().to_string())
            .collect();
        let names = vec![("core-lib".to_string(), "libs/core".to_string())];
        let resolve =
            |values: &[&str]| resolve("--include", &strings(values), &dirs, &root, &names);

        assert_eq!(resolve(&["api"]).unwrap(), strings(&["api"]));
        assert_eq!(resolve(&["./api/"]).unwrap(), strings(&["api"]));
        assert_eq!(resolve(&["."]).unwrap(), strings(&["."]));
        assert_eq!(resolve(&["libs/core"]).unwrap(), strings(&["libs/core"]));
        assert_eq!(resolve(&["core-lib"]).unwrap(), strings(&["libs/core"]));
        assert_eq!(
            resolve(&["api", "api-legacy"]).unwrap(),
            strings(&["api", "api-legacy"])
        );
        // Parts of a name or of the absolute path name nothing
        let absolute = root.join("api").display().to_string();
        for value in ["a", "ap", "core", "ws", absolute.as_str(), ""] {
            let err = resolve(&[value]).unwrap_err().to_string();
            assert!(err.contains("names no repo"), "{value}: {err}");
        }
    }
}
