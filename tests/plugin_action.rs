use std::path::PathBuf;
use std::process::Command;

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("fake-herdr.sh")
}

#[test]
fn should_apply_plugin_action_layout_to_supplied_context_without_creating_workspace() {
    let temp_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let config_path = temp_dir.join("plugin-action-config.yaml");
    let log_path = temp_dir.join("plugin-action-herdr.log");
    let _ = std::fs::remove_file(&log_path);
    std::fs::write(
        &config_path,
        r#"workspaces:
  - name: default
    tabs:
      - label: dev
        panes:
          - {}
          - split: right
            ratio: 0.5
"#,
    )
    .unwrap();

    let output = Command::new(assert_cmd::cargo::cargo_bin!("herdr-spreader"))
        .args(["apply-current", "--file"])
        .arg(&config_path)
        .env("HERDR_BIN_PATH", fixture_path())
        .env("HERDR_WORKSPACE_ID", "w7")
        .env("HERDR_TAB_ID", "w7:t4")
        .env("HERDR_PANE_ID", "w7:p9")
        .env("FAKE_HERDR_LOG", &log_path)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "apply-current failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let log = std::fs::read_to_string(&log_path).unwrap();
    assert_eq!(
        log.lines().collect::<Vec<_>>(),
        vec![
            "pane get w7:p9",
            "pane list --workspace w7",
            "pane move w7:p9 --new-tab --workspace w7 --label dev --focus",
            "tab rename w7:t5 dev",
            "pane move w7:p10 --tab w7:t5 --split right --target-pane w7:p9 --ratio 0.5 --no-focus",
        ]
    );
    assert!(!log.contains("workspace create"));

    let _ = std::fs::remove_file(config_path);
    let _ = std::fs::remove_file(log_path);
}

#[test]
fn plugin_manifest_should_invoke_apply_current() {
    let manifest = include_str!("../herdr-plugin.toml");

    assert!(manifest.contains("\"./target/release/herdr-spreader\","));
    assert!(manifest.contains("\"apply-current\","));
}
