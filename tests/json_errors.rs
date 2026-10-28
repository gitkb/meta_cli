//! A `--json` caller can tell a missing meta config from any other failure.

use std::path::Path;
use std::process::{Command, Output};

fn meta(cwd: &Path, home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_meta"))
        .args(args)
        .current_dir(cwd)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("XDG_CONFIG_HOME", home.join("xdg"))
        .env("XDG_DATA_HOME", home.join("xdg/data"))
        .env("GIT_CONFIG_GLOBAL", home.join("gitconfig"))
        .env("META_DATA_DIR", home.join("data"))
        .output()
        .unwrap()
}

fn json_error_code(output: &Output) -> serde_json::Value {
    assert_eq!(output.status.code(), Some(1));
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let message = value["error"]["message"].as_str().unwrap();
    assert!(
        message.contains("Could not find meta config file"),
        "{message}"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains(message));
    value["error"]["code"].clone()
}

#[test]
fn json_callers_without_a_config_get_a_json_error() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let outside = temp.path().join("not-a-workspace");
    std::fs::create_dir_all(&outside).unwrap();
    for args in [
        vec!["project", "list", "--recursive", "--json"],
        vec!["--json", "git", "worktree", "list"],
        vec!["--json", "exec", "--", "true"],
    ] {
        let output = meta(&outside, &home, &args);
        assert_eq!(json_error_code(&output), "config_not_found", "{args:?}");
    }
    // An explicit --config that doesn't exist is the same error inside a workspace
    let workspace = temp.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::write(workspace.join(".meta.yaml"), "projects: {}\n").unwrap();
    let output = meta(
        &workspace,
        &home,
        &["--config", "absent.yaml", "project", "list", "--json"],
    );
    assert_eq!(json_error_code(&output), "config_not_found");
    // Any other failure is not reported as a missing config
    std::fs::write(workspace.join(".meta.yaml"), "projects: [unclosed\n").unwrap();
    let output = meta(&workspace, &home, &["project", "list", "--json"]);
    assert!(!output.status.success());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("config_not_found"));
}
