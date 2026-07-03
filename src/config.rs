use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Workspace {
    pub name: String,
    #[serde(default)]
    pub root: Option<PathBuf>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub tabs: Vec<Tab>,
    #[serde(default)]
    pub focus: bool,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SpreadFile {
    pub workspaces: Vec<Workspace>,
}

impl SpreadFile {
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Result<Self, serde_yaml_ng::Error> {
        serde_yaml_ng::from_str(s)
    }
}

#[derive(Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Tab {
    pub label: Option<String>,
    #[serde(default)]
    pub cwd: Option<PathBuf>,
    #[serde(default)]
    pub panes: Vec<Pane>,
}

#[derive(Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Pane {
    pub command: Option<String>,
    #[serde(default)]
    pub cwd: Option<PathBuf>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub split: SplitDirection,
    pub ratio: Option<f64>,
    pub wait_for: Option<WaitFor>,
    #[serde(default)]
    pub focus: bool,
}

#[derive(Debug, Default, Clone, Copy, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SplitDirection {
    #[default]
    Right,
    Down,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WaitFor {
    #[serde(rename = "match")]
    pub pattern: String,
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to read config file {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse config file {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_yaml_ng::Error,
    },
}

const CONFIG_FILE_NAME: &str = "spread.yml";

pub fn resolve_config_path(explicit: Option<PathBuf>, env: &BTreeMap<String, String>) -> PathBuf {
    if let Some(path) = explicit {
        return path;
    }

    if let Some(plugin_config_dir) = env.get("HERDR_PLUGIN_CONFIG_DIR") {
        return PathBuf::from(plugin_config_dir).join(CONFIG_FILE_NAME);
    }

    let home = env.get("HOME").map(String::as_str).unwrap_or("");
    PathBuf::from(home)
        .join(".config")
        .join("herdr-spreader")
        .join(CONFIG_FILE_NAME)
}

pub fn load_config(path: &Path) -> Result<SpreadFile, ConfigError> {
    let contents = fs::read_to_string(path).map_err(|source| ConfigError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    SpreadFile::from_str(&contents).map_err(|source| ConfigError::Parse {
        path: path.to_path_buf(),
        source,
    })
}

pub fn resolve_paths(file: SpreadFile, env: &BTreeMap<String, String>, cwd: &Path) -> SpreadFile {
    SpreadFile {
        workspaces: file
            .workspaces
            .into_iter()
            .map(|ws| resolve_workspace_paths(ws, env, cwd))
            .collect(),
    }
}

fn resolve_workspace_paths(
    mut config: Workspace,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Workspace {
    // Default root to the invocation cwd so relative tab/pane cwd values always
    // have an absolute base to resolve against, instead of leaking a bare relative
    // path through to herdr or a shell `cd` when the config has no explicit root.
    let root = config.root.take().unwrap_or_else(|| cwd.to_path_buf());
    config.root = Some(expand_root_path(&root, env, cwd));
    for tab in &mut config.tabs {
        if let Some(tab_cwd) = tab.cwd.take() {
            tab.cwd = Some(expand_tilde(&tab_cwd, env));
        }
        for pane in &mut tab.panes {
            if let Some(pane_cwd) = pane.cwd.take() {
                pane.cwd = Some(expand_tilde(&pane_cwd, env));
            }
        }
    }
    config
}

fn expand_root_path(path: &Path, env: &BTreeMap<String, String>, cwd: &Path) -> PathBuf {
    let expanded = expand_tilde(path, env);
    if expanded.is_absolute() {
        expanded
    } else {
        cwd.join(expanded)
    }
}

fn expand_tilde(path: &Path, env: &BTreeMap<String, String>) -> PathBuf {
    let path_str = path.to_string_lossy();
    let home = env.get("HOME").map(String::as_str).unwrap_or("");
    if path_str == "~" {
        PathBuf::from(home)
    } else if let Some(rest) = path_str.strip_prefix("~/") {
        PathBuf::from(home).join(rest)
    } else {
        path.to_path_buf()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_parse_two_workspaces_given_multi_workspace_yaml() {
        let yaml = r#"
workspaces:
  - name: frontend
  - name: backend
"#;

        let file = SpreadFile::from_str(yaml).unwrap();

        assert_eq!(file.workspaces.len(), 2);
        assert_eq!(file.workspaces[0].name, "frontend");
        assert_eq!(file.workspaces[1].name, "backend");
    }

    #[test]
    fn should_reject_legacy_top_level_single_workspace_yaml() {
        let yaml = "name: demo\ntabs: []";

        let result = SpreadFile::from_str(yaml);

        let err = result.unwrap_err();
        let message = err.to_string();
        assert!(message.contains("name"));
        assert!(message.contains("workspaces"));
    }

    #[test]
    fn should_parse_workspace_level_focus_flag_and_default_it_to_false() {
        let yaml = r#"
workspaces:
  - name: frontend
  - name: backend
    focus: true
"#;

        let file = SpreadFile::from_str(yaml).unwrap();

        assert!(!file.workspaces[0].focus);
        assert!(file.workspaces[1].focus);
    }

    #[test]
    fn should_parse_workspace_name_given_minimal_yaml_with_only_name() {
        let yaml = r#"
workspaces:
  - name: demo
"#;

        let file = SpreadFile::from_str(yaml).unwrap();

        assert_eq!(file.workspaces[0].name, "demo");
        assert!(file.workspaces[0].tabs.is_empty());
        assert_eq!(file.workspaces[0].root, None);
    }

    #[test]
    fn should_parse_tabs_and_panes_given_tmuxinator_style_yaml() {
        let yaml = r#"
workspaces:
  - name: demo
    tabs:
      - label: editor
        panes:
          - command: nvim
      - label: server
        panes:
          - command: cargo run
"#;

        let file = SpreadFile::from_str(yaml).unwrap();

        assert_eq!(file.workspaces[0].tabs[0].label, Some("editor".to_string()));
        assert_eq!(
            file.workspaces[0].tabs[0].panes[0].command,
            Some("nvim".to_string())
        );
        assert_eq!(file.workspaces[0].tabs[1].panes.len(), 1);
    }

    #[test]
    fn should_default_split_to_right_and_ratio_to_none_when_omitted() {
        let yaml = r#"
workspaces:
  - name: demo
    tabs:
      - panes:
          - command: nvim
"#;

        let file = SpreadFile::from_str(yaml).unwrap();

        assert_eq!(
            file.workspaces[0].tabs[0].panes[0].split,
            SplitDirection::Right
        );
        assert!(file.workspaces[0].tabs[0].panes[0].ratio.is_none());
    }

    #[test]
    fn should_reject_config_given_unknown_split_direction() {
        let yaml = r#"
workspaces:
  - name: demo
    tabs:
      - panes:
          - command: nvim
            split: left
"#;

        let result = SpreadFile::from_str(yaml);

        let err = result.unwrap_err();
        assert!(err.to_string().contains("left"));
    }

    #[test]
    fn should_parse_wait_for_with_match_and_timeout_given_pane_sync_config() {
        let yaml = r#"
workspaces:
  - name: demo
    tabs:
      - panes:
          - command: nvim
            wait_for:
              match: "ready"
              timeout_ms: 5000
"#;

        let file = SpreadFile::from_str(yaml).unwrap();

        assert_eq!(
            file.workspaces[0].tabs[0].panes[0].wait_for,
            Some(WaitFor {
                pattern: "ready".to_string(),
                timeout_ms: Some(5000),
            })
        );
    }

    #[test]
    fn should_include_file_path_in_error_when_config_file_is_missing() {
        let path = Path::new("/nonexistent/path/to/spread.yml");

        let result = load_config(path);

        let err = result.unwrap_err();
        assert!(err.to_string().contains("/nonexistent/path/to/spread.yml"));
    }

    #[test]
    fn should_load_spread_file_from_disk_given_multi_workspace_yaml() {
        let yaml = r#"
workspaces:
  - name: frontend
  - name: backend
"#;
        let path = std::env::temp_dir().join(format!(
            "herdr-spreader-test-{}-should_load_spread_file_from_disk_given_multi_workspace_yaml.yml",
            std::process::id()
        ));
        fs::write(&path, yaml).unwrap();

        let file = load_config(&path).unwrap();

        assert_eq!(file.workspaces.len(), 2);

        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn should_prefer_explicit_path_over_plugin_config_dir_when_resolving_config_path() {
        let explicit = Some(PathBuf::from("/tmp/x.yml"));
        let mut env = BTreeMap::new();
        env.insert("HERDR_PLUGIN_CONFIG_DIR".to_string(), "/cfg".to_string());

        let resolved = resolve_config_path(explicit, &env);

        assert_eq!(resolved, PathBuf::from("/tmp/x.yml"));
    }

    #[test]
    fn should_fall_back_to_plugin_config_dir_then_user_config_dir_when_no_explicit_path() {
        let mut env_with_plugin_dir = BTreeMap::new();
        env_with_plugin_dir.insert("HERDR_PLUGIN_CONFIG_DIR".to_string(), "/cfg".to_string());

        let resolved_with_plugin_dir = resolve_config_path(None, &env_with_plugin_dir);
        assert_eq!(resolved_with_plugin_dir, PathBuf::from("/cfg/spread.yml"));

        let mut env_with_home_only = BTreeMap::new();
        env_with_home_only.insert("HOME".to_string(), "/home/demo".to_string());

        let resolved_with_home = resolve_config_path(None, &env_with_home_only);
        assert_eq!(
            resolved_with_home,
            PathBuf::from("/home/demo/.config/herdr-spreader/spread.yml")
        );
    }

    #[test]
    fn should_expand_home_relative_root_given_tilde_slash_prefix() {
        let config = Workspace {
            name: "demo".to_string(),
            root: Some(PathBuf::from("~/code/my-project")),
            ..Default::default()
        };
        let mut env = BTreeMap::new();
        env.insert("HOME".to_string(), "/home/demo".to_string());

        let resolved = resolve_workspace_paths(config, &env, Path::new("/irrelevant"));

        assert_eq!(
            resolved.root,
            Some(PathBuf::from("/home/demo/code/my-project"))
        );
    }

    #[test]
    fn should_expand_bare_tilde_root_to_home_directory() {
        let config = Workspace {
            name: "demo".to_string(),
            root: Some(PathBuf::from("~")),
            ..Default::default()
        };
        let mut env = BTreeMap::new();
        env.insert("HOME".to_string(), "/home/demo".to_string());

        let resolved = resolve_workspace_paths(config, &env, Path::new("/irrelevant"));

        assert_eq!(resolved.root, Some(PathBuf::from("/home/demo")));
    }

    #[test]
    fn should_leave_absolute_root_unchanged() {
        let config = Workspace {
            name: "demo".to_string(),
            root: Some(PathBuf::from("/proj")),
            ..Default::default()
        };

        let resolved = resolve_workspace_paths(config, &BTreeMap::new(), Path::new("/irrelevant"));

        assert_eq!(resolved.root, Some(PathBuf::from("/proj")));
    }

    #[test]
    fn should_resolve_relative_root_against_given_cwd() {
        let config = Workspace {
            name: "demo".to_string(),
            root: Some(PathBuf::from("proj")),
            ..Default::default()
        };

        let resolved = resolve_workspace_paths(config, &BTreeMap::new(), Path::new("/home/demo"));

        assert_eq!(resolved.root, Some(PathBuf::from("/home/demo/proj")));
    }

    #[test]
    fn should_expand_tilde_in_tab_and_pane_cwd_without_forcing_them_absolute() {
        let config = Workspace {
            name: "demo".to_string(),
            root: Some(PathBuf::from("/proj")),
            tabs: vec![Tab {
                label: None,
                cwd: Some(PathBuf::from("~/logs")),
                panes: vec![
                    Pane {
                        command: None,
                        cwd: Some(PathBuf::from("~")),
                        ..Default::default()
                    },
                    Pane {
                        command: None,
                        cwd: Some(PathBuf::from("./relative")),
                        ..Default::default()
                    },
                ],
            }],
            ..Default::default()
        };
        let mut env = BTreeMap::new();
        env.insert("HOME".to_string(), "/home/demo".to_string());

        let resolved = resolve_workspace_paths(config, &env, Path::new("/irrelevant"));

        assert_eq!(resolved.tabs[0].cwd, Some(PathBuf::from("/home/demo/logs")));
        assert_eq!(
            resolved.tabs[0].panes[0].cwd,
            Some(PathBuf::from("/home/demo"))
        );
        assert_eq!(
            resolved.tabs[0].panes[1].cwd,
            Some(PathBuf::from("./relative"))
        );
    }

    #[test]
    fn should_default_root_to_invocation_cwd_when_root_is_absent() {
        let config = Workspace {
            name: "demo".to_string(),
            tabs: vec![Tab {
                label: None,
                cwd: Some(PathBuf::from("svc")),
                panes: vec![],
            }],
            ..Default::default()
        };

        let resolved =
            resolve_workspace_paths(config, &BTreeMap::new(), Path::new("/home/demo/project"));

        assert_eq!(resolved.root, Some(PathBuf::from("/home/demo/project")));
        assert_eq!(resolved.tabs[0].cwd, Some(PathBuf::from("svc")));
    }

    #[test]
    fn should_resolve_each_workspace_root_independently_given_two_workspaces() {
        let file = SpreadFile {
            workspaces: vec![
                Workspace {
                    name: "ws1".to_string(),
                    root: Some(PathBuf::from("~/a")),
                    ..Default::default()
                },
                Workspace {
                    name: "ws2".to_string(),
                    root: Some(PathBuf::from("proj")),
                    ..Default::default()
                },
            ],
        };
        let mut env = BTreeMap::new();
        env.insert("HOME".to_string(), "/home/demo".to_string());

        let resolved = resolve_paths(file, &env, Path::new("/home/demo/base"));

        assert_eq!(
            resolved.workspaces[0].root,
            Some(PathBuf::from("/home/demo/a"))
        );
        assert_eq!(
            resolved.workspaces[1].root,
            Some(PathBuf::from("/home/demo/base/proj"))
        );
    }

    #[test]
    fn should_default_all_roots_to_invocation_cwd_when_multiple_workspaces_omit_root() {
        let file = SpreadFile {
            workspaces: vec![
                Workspace {
                    name: "ws1".to_string(),
                    ..Default::default()
                },
                Workspace {
                    name: "ws2".to_string(),
                    ..Default::default()
                },
            ],
        };

        let resolved = resolve_paths(file, &BTreeMap::new(), Path::new("/home/demo/project"));

        assert_eq!(
            resolved.workspaces[0].root,
            Some(PathBuf::from("/home/demo/project"))
        );
        assert_eq!(
            resolved.workspaces[1].root,
            Some(PathBuf::from("/home/demo/project"))
        );
    }

    #[test]
    fn should_parse_bundled_example_config() {
        let yaml = include_str!("../examples/spread.yml");

        let file = SpreadFile::from_str(yaml).unwrap();

        // Loose on content (the example is free to evolve) but pinned on the
        // schema-level things this test exists to guard: the file parses under
        // `deny_unknown_fields`, describes at least the multi-workspace case,
        // and exercises the `focus: true` feature it's meant to demonstrate.
        assert!(file.workspaces.len() >= 2);
        assert!(file.workspaces.iter().any(|ws| ws.focus));
    }
}
