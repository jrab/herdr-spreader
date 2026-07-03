use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use thiserror::Error;

use crate::backend::{BackendError, HerdrBackend, SplitOpts, TabOpts, WorkspaceOpts};
use crate::config::{SpreadFile, Workspace};

fn resolve_cwd(
    root: Option<&Path>,
    tab_cwd: Option<&Path>,
    pane_cwd: Option<&Path>,
) -> Option<PathBuf> {
    let base = combine_cwd(root, tab_cwd);
    combine_cwd(base.as_deref(), pane_cwd)
}

fn combine_cwd(base: Option<&Path>, overlay: Option<&Path>) -> Option<PathBuf> {
    match (base, overlay) {
        (Some(_base), Some(overlay)) if overlay.is_absolute() => {
            Some(normalize_path(overlay.to_path_buf()))
        }
        (Some(base), Some(overlay)) => Some(normalize_path(base.join(overlay))),
        (Some(base), None) => Some(normalize_path(base.to_path_buf())),
        (None, Some(overlay)) => Some(normalize_path(overlay.to_path_buf())),
        (None, None) => None,
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

fn cwd_env_prefix(cwd: Option<&Path>, env: &BTreeMap<String, String>) -> Option<String> {
    let mut prefix_parts = Vec::new();
    if let Some(cwd) = cwd {
        prefix_parts.push(format!("cd {}", shell_quote(&cwd.display().to_string())));
    }
    for (key, value) in env {
        prefix_parts.push(format!("export {key}={}", shell_quote(value)));
    }
    if prefix_parts.is_empty() {
        None
    } else {
        Some(prefix_parts.join(" && "))
    }
}

fn wrap_command_with_cwd_and_env(
    command: &str,
    cwd: Option<&Path>,
    env: &BTreeMap<String, String>,
) -> String {
    match cwd_env_prefix(cwd, env) {
        Some(prefix) => format!("{prefix} && {command}"),
        None => command.to_string(),
    }
}

fn normalize_path(path: PathBuf) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        if component != Component::CurDir {
            result.push(component.as_os_str());
        }
    }
    result
}

#[derive(Debug, Error)]
pub enum EngineError {
    #[error(transparent)]
    Backend(#[from] BackendError),
    #[error("wait_for was specified on a pane without a command")]
    WaitForWithoutCommand,
}

pub fn apply(file: &SpreadFile, backend: &mut dyn HerdrBackend) -> Result<(), EngineError> {
    let mut chosen: Option<String> = None;
    for ws in &file.workspaces {
        let focus_pane_id = apply_workspace(ws, backend)?;
        if ws.focus || chosen.is_none() {
            chosen = Some(focus_pane_id);
        }
    }
    if let Some(pane_id) = chosen {
        backend.focus_pane(&pane_id)?;
    }
    Ok(())
}

pub fn apply_workspace(
    ws: &Workspace,
    backend: &mut dyn HerdrBackend,
) -> Result<String, EngineError> {
    let workspace = backend.create_workspace(&WorkspaceOpts {
        label: ws.name.clone(),
        cwd: ws.root.clone(),
        env: ws.env.clone(),
        focus: false,
    })?;

    let mut focus_pane_id = workspace.root_pane_id.clone();

    for (index, tab) in ws.tabs.iter().enumerate() {
        let root_pane_id = if index == 0 {
            if let Some(label) = &tab.label {
                backend.rename_tab(&workspace.tab_id, label)?;
            }
            workspace.root_pane_id.clone()
        } else {
            let created_tab = backend.create_tab(
                &workspace.workspace_id,
                &TabOpts {
                    label: tab.label.clone(),
                    cwd: resolve_cwd(ws.root.as_deref(), tab.cwd.as_deref(), None),
                    focus: false,
                },
            )?;
            created_tab.root_pane_id
        };

        let mut previous_pane_id = root_pane_id;

        for (pane_index, pane) in tab.panes.iter().enumerate() {
            let pane_id = if pane_index == 0 {
                previous_pane_id.clone()
            } else {
                backend.split_pane(
                    &previous_pane_id,
                    &SplitOpts {
                        direction: pane.split,
                        ratio: pane.ratio,
                        cwd: resolve_cwd(
                            ws.root.as_deref(),
                            tab.cwd.as_deref(),
                            pane.cwd.as_deref(),
                        ),
                        env: pane.env.clone(),
                        focus: false,
                    },
                )?
            };

            // Tab index 0's pane 0 reuses the workspace's root pane (already at
            // `ws.root`); later tabs' pane 0 reuses the tab's root pane (already
            // resolved against `tab.cwd` in `create_tab` above). Only emit a `cd`
            // when a pane-level override (or, for the first tab, a tab-level
            // override) asks for something beyond that baseline.
            let needs_cwd_override = pane.cwd.is_some() || (index == 0 && tab.cwd.is_some());
            let resolved_cwd = if pane_index == 0 && needs_cwd_override {
                resolve_cwd(ws.root.as_deref(), tab.cwd.as_deref(), pane.cwd.as_deref())
            } else {
                None
            };

            match &pane.command {
                Some(command) => {
                    let command_to_run = if pane_index == 0 {
                        wrap_command_with_cwd_and_env(command, resolved_cwd.as_deref(), &pane.env)
                    } else {
                        command.clone()
                    };

                    backend.run(&pane_id, &command_to_run)?;

                    if let Some(wait_for) = &pane.wait_for {
                        backend.wait_output(&pane_id, wait_for)?;
                    }
                }
                None => {
                    if pane.wait_for.is_some() {
                        return Err(EngineError::WaitForWithoutCommand);
                    }
                    if pane_index == 0
                        && let Some(prefix) = cwd_env_prefix(resolved_cwd.as_deref(), &pane.env)
                    {
                        backend.run(&pane_id, &prefix)?;
                    }
                }
            }

            if pane.focus {
                focus_pane_id = pane_id.clone();
            }

            previous_pane_id = pane_id;
        }
    }

    Ok(focus_pane_id)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use crate::backend::{
        BackendError, HerdrBackend, SplitOpts, TabCreated, TabOpts, WorkspaceCreated, WorkspaceOpts,
    };
    use crate::config::{Pane, SplitDirection, SpreadFile, Tab, WaitFor, Workspace};
    use crate::engine;

    #[derive(Debug, PartialEq)]
    enum Call {
        CreateWorkspace(WorkspaceOpts),
        RenameTab { tab_id: String, label: String },
        CreateTab { workspace_id: String, opts: TabOpts },
        SplitPane { from: String, opts: SplitOpts },
        Run { pane_id: String, command: String },
        WaitOutput { pane_id: String, wait: WaitFor },
        FocusPane { pane_id: String },
    }

    #[derive(Default)]
    struct MockBackend {
        calls: Vec<Call>,
        next_workspace: u32,
        current_ws: String,
        next_tab: u32,
        next_pane: u32,
    }

    impl HerdrBackend for MockBackend {
        fn create_workspace(
            &mut self,
            opts: &WorkspaceOpts,
        ) -> Result<WorkspaceCreated, BackendError> {
            self.calls.push(Call::CreateWorkspace(opts.clone()));
            self.next_workspace += 1;
            self.current_ws = format!("w{}", self.next_workspace);
            self.next_tab = 2;
            self.next_pane = 2;
            Ok(WorkspaceCreated {
                workspace_id: self.current_ws.clone(),
                tab_id: format!("{}:t1", self.current_ws),
                root_pane_id: format!("{}:p1", self.current_ws),
            })
        }

        fn create_tab(
            &mut self,
            workspace_id: &str,
            opts: &TabOpts,
        ) -> Result<TabCreated, BackendError> {
            self.calls.push(Call::CreateTab {
                workspace_id: workspace_id.to_string(),
                opts: opts.clone(),
            });
            let tab_id = format!("{workspace_id}:t{}", self.next_tab);
            let pane_id = format!("{workspace_id}:p{}", self.next_pane);
            self.next_tab += 1;
            self.next_pane += 1;
            Ok(TabCreated {
                tab_id,
                root_pane_id: pane_id,
            })
        }

        fn split_pane(
            &mut self,
            from_pane: &str,
            opts: &SplitOpts,
        ) -> Result<String, BackendError> {
            self.calls.push(Call::SplitPane {
                from: from_pane.to_string(),
                opts: opts.clone(),
            });
            let pane_id = format!("{}:p{}", self.current_ws, self.next_pane);
            self.next_pane += 1;
            Ok(pane_id)
        }

        fn run(&mut self, pane_id: &str, command: &str) -> Result<(), BackendError> {
            self.calls.push(Call::Run {
                pane_id: pane_id.to_string(),
                command: command.to_string(),
            });
            Ok(())
        }

        fn rename_tab(&mut self, tab_id: &str, label: &str) -> Result<(), BackendError> {
            self.calls.push(Call::RenameTab {
                tab_id: tab_id.to_string(),
                label: label.to_string(),
            });
            Ok(())
        }

        fn wait_output(&mut self, pane_id: &str, wait: &WaitFor) -> Result<(), BackendError> {
            self.calls.push(Call::WaitOutput {
                pane_id: pane_id.to_string(),
                wait: wait.clone(),
            });
            Ok(())
        }

        fn focus_pane(&mut self, pane_id: &str) -> Result<(), BackendError> {
            self.calls.push(Call::FocusPane {
                pane_id: pane_id.to_string(),
            });
            Ok(())
        }
    }

    #[test]
    fn should_create_workspace_with_label_cwd_and_no_focus_given_minimal_config() {
        let config = Workspace {
            name: "demo".to_string(),
            root: Some(PathBuf::from("/proj")),
            ..Default::default()
        };
        let mut mock = MockBackend::default();

        engine::apply_workspace(&config, &mut mock).unwrap();

        assert_eq!(
            mock.calls[0],
            Call::CreateWorkspace(WorkspaceOpts {
                label: "demo".to_string(),
                cwd: Some(PathBuf::from("/proj")),
                env: BTreeMap::new(),
                focus: false,
            })
        );
    }

    #[test]
    fn should_run_command_in_root_pane_given_single_tab_with_one_pane() {
        let config = Workspace {
            name: "demo".to_string(),
            tabs: vec![Tab {
                label: None,
                panes: vec![Pane {
                    command: Some("nvim".to_string()),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut mock = MockBackend::default();

        engine::apply_workspace(&config, &mut mock).unwrap();

        assert_eq!(
            mock.calls[1],
            Call::Run {
                pane_id: "w1:p1".to_string(),
                command: "nvim".to_string(),
            }
        );
    }

    #[test]
    fn should_rename_root_tab_when_first_tab_has_label() {
        let config = Workspace {
            name: "demo".to_string(),
            tabs: vec![Tab {
                label: Some("editor".to_string()),
                panes: vec![Pane {
                    command: Some("nvim".to_string()),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut mock = MockBackend::default();

        engine::apply_workspace(&config, &mut mock).unwrap();

        let rename_index = mock
            .calls
            .iter()
            .position(|c| matches!(c, Call::RenameTab { .. }))
            .expect("rename_tab was not called");
        let run_index = mock
            .calls
            .iter()
            .position(|c| matches!(c, Call::Run { .. }))
            .expect("run was not called");

        assert_eq!(
            mock.calls[rename_index],
            Call::RenameTab {
                tab_id: "w1:t1".to_string(),
                label: "editor".to_string(),
            }
        );
        assert!(rename_index < run_index);
    }

    #[test]
    fn should_create_additional_tab_threading_workspace_id_given_second_tab() {
        let config = Workspace {
            name: "demo".to_string(),
            tabs: vec![
                Tab {
                    label: None,
                    panes: vec![Pane {
                        command: None,
                        ..Default::default()
                    }],
                    ..Default::default()
                },
                Tab {
                    label: Some("server".to_string()),
                    panes: vec![Pane {
                        command: Some("cargo run".to_string()),
                        ..Default::default()
                    }],
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let mut mock = MockBackend::default();

        engine::apply_workspace(&config, &mut mock).unwrap();

        let create_tab_index = mock
            .calls
            .iter()
            .position(|c| matches!(c, Call::CreateTab { .. }))
            .expect("create_tab was not called");

        assert_eq!(
            mock.calls[create_tab_index],
            Call::CreateTab {
                workspace_id: "w1".to_string(),
                opts: TabOpts {
                    label: Some("server".to_string()),
                    cwd: None,
                    focus: false,
                },
            }
        );
        assert_eq!(
            mock.calls[create_tab_index + 1],
            Call::Run {
                pane_id: "w1:p2".to_string(),
                command: "cargo run".to_string(),
            }
        );
    }

    #[test]
    fn should_split_from_previous_pane_with_direction_and_ratio_given_multi_pane_tab() {
        let config = Workspace {
            name: "demo".to_string(),
            tabs: vec![Tab {
                label: None,
                panes: vec![
                    Pane {
                        command: None,
                        ..Default::default()
                    },
                    Pane {
                        command: Some("watch".to_string()),
                        split: SplitDirection::Down,
                        ratio: Some(0.3),
                        ..Default::default()
                    },
                    Pane {
                        command: Some("logs".to_string()),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut mock = MockBackend::default();

        engine::apply_workspace(&config, &mut mock).unwrap();

        assert_eq!(
            mock.calls[1],
            Call::SplitPane {
                from: "w1:p1".to_string(),
                opts: SplitOpts {
                    direction: SplitDirection::Down,
                    ratio: Some(0.3),
                    ..Default::default()
                },
            }
        );
        assert_eq!(
            mock.calls[2],
            Call::Run {
                pane_id: "w1:p2".to_string(),
                command: "watch".to_string(),
            }
        );
        assert_eq!(
            mock.calls[3],
            Call::SplitPane {
                from: "w1:p2".to_string(),
                opts: SplitOpts {
                    direction: SplitDirection::Right,
                    ratio: None,
                    ..Default::default()
                },
            }
        );
        assert_eq!(
            mock.calls[4],
            Call::Run {
                pane_id: "w1:p3".to_string(),
                command: "logs".to_string(),
            }
        );
    }

    #[test]
    fn should_pass_pane_cwd_and_env_to_split_opts_given_pane_overrides() {
        let mut pane_env = BTreeMap::new();
        pane_env.insert("FOO".to_string(), "bar".to_string());

        let config = Workspace {
            name: "demo".to_string(),
            root: Some(PathBuf::from("/proj")),
            tabs: vec![Tab {
                label: None,
                panes: vec![
                    Pane {
                        command: None,
                        ..Default::default()
                    },
                    Pane {
                        command: Some("watch".to_string()),
                        cwd: Some(PathBuf::from("./sub")),
                        env: pane_env.clone(),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut mock = MockBackend::default();

        engine::apply_workspace(&config, &mut mock).unwrap();

        assert_eq!(
            mock.calls[1],
            Call::SplitPane {
                from: "w1:p1".to_string(),
                opts: SplitOpts {
                    direction: SplitDirection::Right,
                    ratio: None,
                    cwd: Some(PathBuf::from("/proj/sub")),
                    env: pane_env,
                    focus: false,
                },
            }
        );
    }

    #[test]
    fn should_call_wait_output_after_run_and_before_next_pane_given_wait_for() {
        let config = Workspace {
            name: "demo".to_string(),
            tabs: vec![Tab {
                label: None,
                panes: vec![
                    Pane {
                        command: Some("watch".to_string()),
                        wait_for: Some(WaitFor {
                            pattern: "ready".to_string(),
                            timeout_ms: None,
                        }),
                        ..Default::default()
                    },
                    Pane {
                        command: Some("logs".to_string()),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut mock = MockBackend::default();

        engine::apply_workspace(&config, &mut mock).unwrap();

        assert_eq!(
            mock.calls[1],
            Call::Run {
                pane_id: "w1:p1".to_string(),
                command: "watch".to_string(),
            }
        );
        assert_eq!(
            mock.calls[2],
            Call::WaitOutput {
                pane_id: "w1:p1".to_string(),
                wait: WaitFor {
                    pattern: "ready".to_string(),
                    timeout_ms: None,
                },
            }
        );
        assert_eq!(
            mock.calls[3],
            Call::SplitPane {
                from: "w1:p1".to_string(),
                opts: SplitOpts {
                    direction: SplitDirection::Right,
                    ratio: None,
                    ..Default::default()
                },
            }
        );
    }

    #[test]
    fn should_wrap_first_pane_command_with_cd_and_env_export_given_root_tab_and_pane_overrides() {
        let mut pane_env = BTreeMap::new();
        pane_env.insert("FOO".to_string(), "bar".to_string());

        let config = Workspace {
            name: "demo".to_string(),
            root: Some(PathBuf::from("/proj")),
            tabs: vec![Tab {
                label: None,
                cwd: Some(PathBuf::from("./sub")),
                panes: vec![Pane {
                    command: Some("nvim".to_string()),
                    cwd: Some(PathBuf::from("./inner")),
                    env: pane_env,
                    ..Default::default()
                }],
            }],
            ..Default::default()
        };
        let mut mock = MockBackend::default();

        engine::apply_workspace(&config, &mut mock).unwrap();

        assert_eq!(
            mock.calls[1],
            Call::Run {
                pane_id: "w1:p1".to_string(),
                command: "cd '/proj/sub/inner' && export FOO='bar' && nvim".to_string(),
            }
        );
    }

    #[test]
    fn should_run_bare_cd_and_export_on_first_pane_given_no_command_but_cwd_and_env_set() {
        let mut pane_env = BTreeMap::new();
        pane_env.insert("FOO".to_string(), "bar".to_string());

        let config = Workspace {
            name: "demo".to_string(),
            root: Some(PathBuf::from("/proj")),
            tabs: vec![Tab {
                label: None,
                cwd: Some(PathBuf::from("./sub")),
                panes: vec![Pane {
                    command: None,
                    env: pane_env,
                    ..Default::default()
                }],
            }],
            ..Default::default()
        };
        let mut mock = MockBackend::default();

        engine::apply_workspace(&config, &mut mock).unwrap();

        assert_eq!(
            mock.calls[1],
            Call::Run {
                pane_id: "w1:p1".to_string(),
                command: "cd '/proj/sub' && export FOO='bar'".to_string(),
            }
        );
    }

    #[test]
    fn should_not_call_run_on_first_pane_given_no_command_and_no_cwd_or_env() {
        let config = Workspace {
            name: "demo".to_string(),
            tabs: vec![Tab {
                label: None,
                panes: vec![Pane {
                    command: None,
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut mock = MockBackend::default();

        engine::apply_workspace(&config, &mut mock).unwrap();

        assert!(!mock.calls.iter().any(|c| matches!(c, Call::Run { .. })));
    }

    #[test]
    fn should_resolve_tab_cwd_against_root_given_second_tab_with_relative_cwd() {
        let config = Workspace {
            name: "demo".to_string(),
            root: Some(PathBuf::from("/proj")),
            tabs: vec![
                Tab {
                    label: None,
                    panes: vec![Pane {
                        command: None,
                        ..Default::default()
                    }],
                    ..Default::default()
                },
                Tab {
                    label: Some("server".to_string()),
                    cwd: Some(PathBuf::from("./svc")),
                    panes: vec![Pane {
                        command: Some("cargo run".to_string()),
                        ..Default::default()
                    }],
                },
            ],
            ..Default::default()
        };
        let mut mock = MockBackend::default();

        engine::apply_workspace(&config, &mut mock).unwrap();

        let create_tab_index = mock
            .calls
            .iter()
            .position(|c| matches!(c, Call::CreateTab { .. }))
            .expect("create_tab was not called");

        assert_eq!(
            mock.calls[create_tab_index],
            Call::CreateTab {
                workspace_id: "w1".to_string(),
                opts: TabOpts {
                    label: Some("server".to_string()),
                    cwd: Some(PathBuf::from("/proj/svc")),
                    focus: false,
                },
            }
        );
    }

    #[test]
    fn should_error_when_wait_for_is_set_on_a_pane_without_a_command() {
        let config = Workspace {
            name: "demo".to_string(),
            tabs: vec![Tab {
                label: None,
                panes: vec![Pane {
                    command: None,
                    wait_for: Some(WaitFor {
                        pattern: "ready".to_string(),
                        timeout_ms: None,
                    }),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut mock = MockBackend::default();

        let result = engine::apply_workspace(&config, &mut mock);

        assert!(matches!(
            result,
            Err(engine::EngineError::WaitForWithoutCommand)
        ));
    }

    #[test]
    fn should_focus_marked_pane_at_end_and_fall_back_to_first_pane_when_none_marked() {
        let config_with_explicit_focus = Workspace {
            name: "demo".to_string(),
            tabs: vec![
                Tab {
                    label: None,
                    panes: vec![Pane {
                        command: None,
                        ..Default::default()
                    }],
                    ..Default::default()
                },
                Tab {
                    label: Some("server".to_string()),
                    panes: vec![Pane {
                        command: Some("cargo run".to_string()),
                        focus: true,
                        ..Default::default()
                    }],
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let file_with_explicit_focus = SpreadFile {
            workspaces: vec![config_with_explicit_focus],
        };
        let mut mock = MockBackend::default();

        engine::apply(&file_with_explicit_focus, &mut mock).unwrap();

        assert_eq!(
            mock.calls.last(),
            Some(&Call::FocusPane {
                pane_id: "w1:p2".to_string(),
            })
        );

        let config_without_focus = Workspace {
            name: "demo".to_string(),
            tabs: vec![Tab {
                label: None,
                panes: vec![Pane {
                    command: Some("nvim".to_string()),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let file_without_focus = SpreadFile {
            workspaces: vec![config_without_focus],
        };
        let mut mock = MockBackend::default();

        engine::apply(&file_without_focus, &mut mock).unwrap();

        assert_eq!(
            mock.calls.last(),
            Some(&Call::FocusPane {
                pane_id: "w1:p1".to_string(),
            })
        );
    }

    #[test]
    fn should_apply_workspaces_in_order_and_focus_first_workspace_pane_once_when_no_focus_flag() {
        let ws1 = Workspace {
            name: "alpha".to_string(),
            tabs: vec![Tab {
                label: None,
                panes: vec![Pane {
                    command: Some("nvim".to_string()),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let ws2 = Workspace {
            name: "beta".to_string(),
            tabs: vec![Tab {
                label: None,
                panes: vec![Pane {
                    command: Some("cargo run".to_string()),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let file = SpreadFile {
            workspaces: vec![ws1, ws2],
        };
        let mut mock = MockBackend::default();

        engine::apply(&file, &mut mock).unwrap();

        let create_workspace_calls: Vec<&Call> = mock
            .calls
            .iter()
            .filter(|c| matches!(c, Call::CreateWorkspace(_)))
            .collect();
        assert_eq!(create_workspace_calls.len(), 2);
        assert_eq!(
            create_workspace_calls[0],
            &Call::CreateWorkspace(WorkspaceOpts {
                label: "alpha".to_string(),
                cwd: None,
                env: BTreeMap::new(),
                focus: false,
            })
        );
        assert_eq!(
            create_workspace_calls[1],
            &Call::CreateWorkspace(WorkspaceOpts {
                label: "beta".to_string(),
                cwd: None,
                env: BTreeMap::new(),
                focus: false,
            })
        );

        let focus_calls: Vec<&Call> = mock
            .calls
            .iter()
            .filter(|c| matches!(c, Call::FocusPane { .. }))
            .collect();
        assert_eq!(focus_calls.len(), 1);
        assert_eq!(
            mock.calls.last(),
            Some(&Call::FocusPane {
                pane_id: "w1:p1".to_string(),
            })
        );
    }

    #[test]
    fn should_focus_pane_inside_workspace_marked_focus_true_given_focus_on_second_workspace() {
        let ws1 = Workspace {
            name: "alpha".to_string(),
            tabs: vec![Tab {
                label: None,
                panes: vec![Pane {
                    command: Some("nvim".to_string()),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let ws2 = Workspace {
            name: "beta".to_string(),
            focus: true,
            tabs: vec![Tab {
                label: None,
                panes: vec![
                    Pane {
                        command: None,
                        ..Default::default()
                    },
                    Pane {
                        command: Some("cargo run".to_string()),
                        focus: true,
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }],
            ..Default::default()
        };
        let file = SpreadFile {
            workspaces: vec![ws1, ws2],
        };
        let mut mock = MockBackend::default();

        engine::apply(&file, &mut mock).unwrap();

        let focus_calls: Vec<&Call> = mock
            .calls
            .iter()
            .filter(|c| matches!(c, Call::FocusPane { .. }))
            .collect();
        assert_eq!(focus_calls.len(), 1);
        assert_eq!(
            mock.calls.last(),
            Some(&Call::FocusPane {
                pane_id: "w2:p2".to_string(),
            })
        );
    }

    #[test]
    fn should_focus_last_workspace_marked_focus_true_when_multiple_workspaces_set_focus() {
        let ws1 = Workspace {
            name: "alpha".to_string(),
            focus: true,
            tabs: vec![Tab {
                label: None,
                panes: vec![Pane {
                    command: Some("nvim".to_string()),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let ws2 = Workspace {
            name: "beta".to_string(),
            tabs: vec![Tab {
                label: None,
                panes: vec![Pane {
                    command: Some("cargo run".to_string()),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let ws3 = Workspace {
            name: "gamma".to_string(),
            focus: true,
            tabs: vec![Tab {
                label: None,
                panes: vec![Pane {
                    command: Some("logs".to_string()),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let file = SpreadFile {
            workspaces: vec![ws1, ws2, ws3],
        };
        let mut mock = MockBackend::default();

        engine::apply(&file, &mut mock).unwrap();

        assert_eq!(
            mock.calls.last(),
            Some(&Call::FocusPane {
                pane_id: "w3:p1".to_string(),
            })
        );
    }

    #[test]
    fn should_make_no_backend_calls_given_empty_workspaces_list() {
        let file = SpreadFile { workspaces: vec![] };
        let mut mock = MockBackend::default();

        let result = engine::apply(&file, &mut mock);

        assert!(result.is_ok());
        assert!(mock.calls.is_empty());
    }

    #[test]
    fn should_return_marked_focus_pane_id_without_calling_focus_pane_when_applying_one_workspace() {
        let ws = Workspace {
            name: "demo".to_string(),
            tabs: vec![Tab {
                label: None,
                panes: vec![
                    Pane {
                        command: None,
                        ..Default::default()
                    },
                    Pane {
                        command: Some("watch".to_string()),
                        focus: true,
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut mock = MockBackend::default();

        let id = engine::apply_workspace(&ws, &mut mock).unwrap();

        assert_eq!(id, "w1:p2");
        assert!(
            !mock
                .calls
                .iter()
                .any(|c| matches!(c, Call::FocusPane { .. }))
        );
    }
}
