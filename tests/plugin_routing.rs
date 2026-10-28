//! `meta exec` runs commands as typed; plugins only get `meta <their command>`.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use tempfile::tempdir;

/// A workspace with repos `a` and `b`, and a plugin on PATH that claims
/// `touch` and logs each request it receives instead of running it.
fn workspace(root: &Path) {
    fs::write(
        root.join(".meta.yaml"),
        r#"{"projects":{"a":"https://example.invalid/a.git","b":"https://example.invalid/b.git"}}"#,
    )
    .unwrap();
    fs::create_dir_all(root.join("a")).unwrap();
    fs::create_dir_all(root.join("b")).unwrap();
    let bin = root.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let plugin = bin.join("meta-touchy");
    fs::write(
        &plugin,
        format!(
            r#"#!/bin/sh
case "$1" in
  --meta-plugin-info) echo '{{"name":"touchy","version":"1.0.0","commands":["touch"]}}' ;;
  --meta-plugin-exec) cat >> "{}" ;;
esac
"#,
            root.join("plugin.log").display()
        ),
    )
    .unwrap();
    fs::set_permissions(&plugin, fs::Permissions::from_mode(0o755)).unwrap();
}

fn meta(root: &Path, args: &[&str]) {
    let output = run_meta(root, args);
    assert!(
        output.status.success(),
        "meta {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn run_meta(root: &Path, args: &[&str]) -> std::process::Output {
    run_meta_in(root, root, args)
}

fn run_meta_in(root: &Path, cwd: &Path, args: &[&str]) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_meta"))
        .args(args)
        .current_dir(cwd)
        .env("HOME", root)
        .env("XDG_CONFIG_HOME", root.join("xdg"))
        .env("GIT_CONFIG_GLOBAL", root.join("gitconfig"))
        .env("META_DATA_DIR", root.join("data"))
        .env(
            "PATH",
            std::env::join_paths(
                std::iter::once(root.join("bin"))
                    .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
            )
            .unwrap(),
        )
        .output()
        .unwrap()
}

#[test]
fn exec_runs_a_plugin_claimed_command_verbatim() {
    let temp = tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    workspace(&root);

    meta(&root, &["exec", "--", "touch", "ran"]);

    for repo in [&root, &root.join("a"), &root.join("b")] {
        assert!(repo.join("ran").exists(), "not run in {}", repo.display());
    }
    assert!(
        !root.join("plugin.log").exists(),
        "meta exec handed the command to the plugin"
    );
}

#[test]
fn plugin_subcommand_still_routes_to_the_plugin() {
    let temp = tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    workspace(&root);

    meta(&root, &["touch", "ran"]);

    let request = fs::read_to_string(root.join("plugin.log")).unwrap();
    assert!(request.contains(r#""command":"touch""#), "{request}");
    assert!(request.contains(r#""args":["ran"]"#), "{request}");
    assert!(
        !root.join("ran").exists(),
        "meta ran the plugin's command raw"
    );
}

#[test]
fn a_plan_meta_cannot_read_fails_and_names_the_plugin() {
    let temp = tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    workspace(&root);
    // An older plugin that still plans `cmd` strings instead of `argv`
    let plugin = root.join("bin").join("meta-oldplan");
    fs::write(
        &plugin,
        r#"#!/bin/sh
case "$1" in
  --meta-plugin-info) echo '{"name":"oldplan","version":"1.0.0","commands":["oldplan"]}' ;;
  --meta-plugin-exec) cat >/dev/null; echo '{"plan":{"commands":[{"dir":".","cmd":"touch ran"}]}}' ;;
esac
"#,
    )
    .unwrap();
    fs::set_permissions(&plugin, fs::Permissions::from_mode(0o755)).unwrap();

    let output = run_meta(&root, &["oldplan", "run"]);

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "an unreadable plan passed as success"
    );
    assert!(stderr.contains("upgrade the plugin"), "{stderr}");
    assert!(stderr.contains(&plugin.display().to_string()), "{stderr}");
}

#[test]
fn a_plugin_command_in_a_plain_git_worktree_needs_a_config() {
    let temp = tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    workspace(&root);
    fs::remove_file(root.join(".meta.yaml")).unwrap();
    // A plain Git worktree under `.worktrees`, with no meta config anywhere
    let repo = root.join("plain");
    fs::create_dir_all(&repo).unwrap();
    for args in [
        &["init", "-q"][..],
        &["commit", "-q", "--allow-empty", "-m", "init"],
        &["worktree", "add", "-q", ".worktrees/x"],
    ] {
        let status = std::process::Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid"])
            .args(["-c", "commit.gpgSign=false"])
            .args(args)
            .current_dir(&repo)
            .env("GIT_CONFIG_GLOBAL", root.join("gitconfig"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    }
    let set = repo.join(".worktrees/x");

    let output = run_meta_in(&root, &set, &["--json", "touch", "file"]);
    assert_eq!(output.status.code(), Some(1));
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["error"]["code"], "config_not_found");
    assert!(!set.join("file").exists(), "ran the plugin's command raw");
    assert!(!root.join("plugin.log").exists());

    // `meta exec` still runs in the set's checkout
    let output = run_meta_in(&root, &set, &["exec", "--", "touch", "ran"]);
    assert!(output.status.success());
    assert!(set.join("ran").exists());
}
