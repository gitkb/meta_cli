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
        root.join(".meta"),
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
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_meta"))
        .args(args)
        .current_dir(root)
        .env("HOME", root)
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
        .unwrap();
    assert!(
        output.status.success(),
        "meta {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
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
