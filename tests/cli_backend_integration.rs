//! Integration test for Unit 3B / Step 21: `CliBackend` against a fake
//! `herdr` CLI.
//!
//! This is an external `tests/` crate, so it links against the *library*
//! target of `herdr-spreader` (see `src/lib.rs`) rather than the binary.
//!
//! The fake herdr (`tests/fixtures/fake-herdr.sh`) echoes canned JSON
//! matching the real herdr response shapes (see
//! `.claude/user/plan/task-graph.md`, section "3. 共有コンテキスト") based on
//! the subcommand it receives, and appends the argv it was called with to a
//! log file named by `$FAKE_HERDR_LOG`. We drive a `SpreadFile` with two
//! workspaces (a 2-tab/3-pane `demo` workspace and a minimal single-pane
//! `demo2` workspace) through `engine::apply` against a `CliBackend` pointed
//! at that fake script, then assert the logged argv sequence matches the
//! expected order: ids are threaded correctly within each workspace (e.g.
//! the second tab's `pane run` targets the pane id returned by that tab's
//! own `create_tab` response, and the mid-tab split's `pane run` targets the
//! pane id returned by `split_pane`, not a fabricated one), the fake herdr
//! script hands out a fresh workspace id (`wA` then `wB`) per `workspace
//! create` call, and `engine::apply` calls `focus_pane` exactly once, at the
//! very end, targeting the second workspace's root pane (since `demo2` sets
//! workspace-level `focus: true` and has no pane-level focus override).

use std::path::PathBuf;

use herdr_spreader::backend::cli::CliBackend;
use herdr_spreader::config::{Pane, SplitDirection, SpreadFile, Tab, WaitFor, Workspace};
use herdr_spreader::engine;

fn fake_herdr_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("fake-herdr.sh")
}

fn log_path() -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("fake_herdr_log_cli_backend_integration.txt")
}

fn build_spread_file() -> SpreadFile {
    SpreadFile {
        workspaces: vec![
            Workspace {
                name: "demo".to_string(),
                tabs: vec![
                    Tab {
                        label: Some("editor".to_string()),
                        cwd: None,
                        panes: vec![
                            Pane {
                                command: Some("nvim".to_string()),
                                focus: true,
                                ..Default::default()
                            },
                            Pane {
                                command: Some("cargo watch -x test".to_string()),
                                split: SplitDirection::Down,
                                ratio: Some(0.3),
                                wait_for: Some(WaitFor {
                                    pattern: "Compiling".to_string(),
                                    timeout_ms: Some(10000),
                                }),
                                ..Default::default()
                            },
                        ],
                    },
                    Tab {
                        label: Some("server".to_string()),
                        cwd: None,
                        panes: vec![Pane {
                            command: Some("cargo run".to_string()),
                            ..Default::default()
                        }],
                    },
                ],
                ..Default::default()
            },
            Workspace {
                name: "demo2".to_string(),
                focus: true,
                tabs: vec![Tab {
                    label: None,
                    cwd: None,
                    panes: vec![Pane {
                        command: Some("htop".to_string()),
                        ..Default::default()
                    }],
                }],
                ..Default::default()
            },
        ],
    }
}

#[test]
fn should_thread_ids_and_focus_once_across_two_workspaces_against_fake_herdr() {
    let log_path = log_path();
    let _ = std::fs::remove_file(&log_path);

    // Safety: this test does not run concurrently with other code in this
    // process that reads/writes HERDR-related env vars.
    unsafe {
        std::env::set_var("FAKE_HERDR_LOG", &log_path);
    }

    let file = build_spread_file();
    let mut backend = CliBackend::new(fake_herdr_path());

    engine::apply(&file, &mut backend).expect("apply against fake herdr should succeed");

    let log_contents = std::fs::read_to_string(&log_path).expect("fake herdr log should exist");
    let logged_lines: Vec<&str> = log_contents.lines().collect();

    let expected_lines = vec![
        "workspace create --label demo --no-focus",
        "tab rename wA:t1 editor",
        "pane run wA:p1 nvim",
        "pane split wA:p1 --direction down --ratio 0.3 --no-focus",
        "pane run wA:p3 cargo watch -x test",
        "wait output wA:p3 --match Compiling --timeout 10000",
        "tab create --workspace wA --label server --no-focus",
        "pane run wA:p2 cargo run",
        "workspace create --label demo2 --no-focus",
        "pane run wB:p1 htop",
        "pane focus --pane wB:p1 --direction left",
    ];

    assert_eq!(logged_lines, expected_lines);

    let _ = std::fs::remove_file(&log_path);
}
