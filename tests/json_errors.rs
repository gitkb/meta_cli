//! A `--json` caller can tell a missing meta config from any other failure.

use std::process::Command;

#[test]
fn json_callers_without_a_config_get_a_json_error() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let cwd = temp.path().join("not-a-workspace");
    std::fs::create_dir_all(&cwd).unwrap();
    for args in [
        vec!["project", "list", "--recursive", "--json"],
        vec!["--json", "git", "worktree", "list"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_meta"))
            .args(&args)
            .current_dir(&cwd)
            .env("HOME", &home)
            .env("USERPROFILE", &home)
            .env("XDG_CONFIG_HOME", home.join("xdg"))
            .env("XDG_DATA_HOME", home.join("xdg/data"))
            .env("GIT_CONFIG_GLOBAL", home.join("gitconfig"))
            .env("META_DATA_DIR", temp.path().join("data"))
            .output()
            .unwrap();
        assert!(!output.status.success(), "{args:?}");
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["error"]["code"], "config_not_found", "{args:?}");
        let message = value["error"]["message"].as_str().unwrap();
        assert!(
            message.contains("Could not find meta config file"),
            "{message}"
        );
        assert!(String::from_utf8_lossy(&output.stderr).contains(message));
    }
}
