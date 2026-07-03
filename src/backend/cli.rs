use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Deserialize;

use super::{
    BackendError, HerdrBackend, SplitOpts, TabCreated, TabOpts, WorkspaceCreated, WorkspaceOpts,
};
use crate::config::{SplitDirection, WaitFor};

pub(crate) fn workspace_create_args(opts: &WorkspaceOpts) -> Vec<String> {
    let mut args = vec!["workspace".to_string(), "create".to_string()];
    push_cwd(&mut args, &opts.cwd);
    args.push("--label".to_string());
    args.push(opts.label.clone());
    push_env(&mut args, &opts.env);
    push_focus_flag(&mut args, opts.focus);
    args
}

pub(crate) fn tab_create_args(workspace_id: &str, opts: &TabOpts) -> Vec<String> {
    let mut args = vec![
        "tab".to_string(),
        "create".to_string(),
        "--workspace".to_string(),
        workspace_id.to_string(),
    ];
    push_cwd(&mut args, &opts.cwd);
    if let Some(label) = &opts.label {
        args.push("--label".to_string());
        args.push(label.clone());
    }
    push_focus_flag(&mut args, opts.focus);
    args
}

pub(crate) fn pane_split_args(from_pane: &str, opts: &SplitOpts) -> Vec<String> {
    let mut args = vec![
        "pane".to_string(),
        "split".to_string(),
        from_pane.to_string(),
        "--direction".to_string(),
        direction_str(opts.direction).to_string(),
    ];
    if let Some(ratio) = opts.ratio {
        args.push("--ratio".to_string());
        args.push(ratio.to_string());
    }
    push_cwd(&mut args, &opts.cwd);
    push_env(&mut args, &opts.env);
    push_focus_flag(&mut args, opts.focus);
    args
}

pub(crate) fn pane_run_args(pane_id: &str, command: &str) -> Vec<String> {
    vec![
        "pane".to_string(),
        "run".to_string(),
        pane_id.to_string(),
        command.to_string(),
    ]
}

pub(crate) fn wait_output_args(pane_id: &str, wait: &WaitFor) -> Vec<String> {
    let mut args = vec![
        "wait".to_string(),
        "output".to_string(),
        pane_id.to_string(),
        "--match".to_string(),
        wait.pattern.clone(),
    ];
    if let Some(timeout_ms) = wait.timeout_ms {
        args.push("--timeout".to_string());
        args.push(timeout_ms.to_string());
    }
    args
}

pub(crate) fn focus_args(pane_id: &str) -> Vec<String> {
    vec!["pane".to_string(), "focus".to_string(), pane_id.to_string()]
}

pub(crate) fn rename_tab_args(tab_id: &str, label: &str) -> Vec<String> {
    vec![
        "tab".to_string(),
        "rename".to_string(),
        tab_id.to_string(),
        label.to_string(),
    ]
}

fn direction_str(direction: SplitDirection) -> &'static str {
    match direction {
        SplitDirection::Right => "right",
        SplitDirection::Down => "down",
    }
}

fn push_cwd(args: &mut Vec<String>, cwd: &Option<PathBuf>) {
    if let Some(cwd) = cwd {
        args.push("--cwd".to_string());
        args.push(cwd.display().to_string());
    }
}

fn push_env(args: &mut Vec<String>, env: &BTreeMap<String, String>) {
    for (key, value) in env {
        args.push("--env".to_string());
        args.push(format!("{key}={value}"));
    }
}

fn push_focus_flag(args: &mut Vec<String>, focus: bool) {
    if !focus {
        args.push("--no-focus".to_string());
    }
}

#[derive(Debug, Deserialize)]
struct ErrorBody {
    message: String,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Envelope<T> {
    Error { error: ErrorBody },
    Ok { result: T },
}

fn parse_envelope<T: for<'de> Deserialize<'de>>(json: &str) -> Result<T, BackendError> {
    let envelope: Envelope<T> =
        serde_json::from_str(json).map_err(|source| BackendError::Herdr {
            message: source.to_string(),
        })?;
    match envelope {
        Envelope::Error { error } => Err(BackendError::Herdr {
            message: error.message,
        }),
        Envelope::Ok { result } => Ok(result),
    }
}

#[derive(Debug, Deserialize)]
struct WorkspaceIdBody {
    workspace_id: String,
}

#[derive(Debug, Deserialize)]
struct TabIdBody {
    tab_id: String,
}

#[derive(Debug, Deserialize)]
struct PaneIdBody {
    pane_id: String,
}

#[derive(Debug, Deserialize)]
struct WorkspaceCreatedBody {
    workspace: WorkspaceIdBody,
    tab: TabIdBody,
    root_pane: PaneIdBody,
}

pub(crate) fn parse_workspace_created(json: &str) -> Result<WorkspaceCreated, BackendError> {
    let body: WorkspaceCreatedBody = parse_envelope(json)?;
    Ok(WorkspaceCreated {
        workspace_id: body.workspace.workspace_id,
        tab_id: body.tab.tab_id,
        root_pane_id: body.root_pane.pane_id,
    })
}

#[derive(Debug, Deserialize)]
struct TabCreatedBody {
    tab: TabIdBody,
    root_pane: PaneIdBody,
}

pub(crate) fn parse_tab_created(json: &str) -> Result<TabCreated, BackendError> {
    let body: TabCreatedBody = parse_envelope(json)?;
    Ok(TabCreated {
        tab_id: body.tab.tab_id,
        root_pane_id: body.root_pane.pane_id,
    })
}

#[derive(Debug, Deserialize)]
struct PaneInfoBody {
    pane: PaneIdBody,
}

pub(crate) fn parse_pane_split(json: &str) -> Result<String, BackendError> {
    let body: PaneInfoBody = parse_envelope(json)?;
    Ok(body.pane.pane_id)
}

pub(crate) fn pane_get_args(pane_id: &str) -> Vec<String> {
    vec!["pane".to_string(), "get".to_string(), pane_id.to_string()]
}

#[derive(Debug, Deserialize)]
struct PaneCwdBody {
    pane: PaneCwdInfo,
}

#[derive(Debug, Deserialize)]
struct PaneCwdInfo {
    #[serde(default)]
    foreground_cwd: Option<String>,
}

pub(crate) fn parse_pane_cwd(json: &str) -> Result<Option<PathBuf>, BackendError> {
    let body: PaneCwdBody = parse_envelope(json)?;
    Ok(body.pane.foreground_cwd.map(PathBuf::from))
}

const DEFAULT_HERDR_BIN: &str = "herdr";

pub struct CliBackend {
    bin: PathBuf,
}

impl CliBackend {
    pub fn new(bin: PathBuf) -> Self {
        Self { bin }
    }

    pub fn resolve_bin(env: &BTreeMap<String, String>) -> PathBuf {
        env.get("HERDR_BIN_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_HERDR_BIN))
    }

    fn exec(&self, args: &[String]) -> Result<String, BackendError> {
        let output = std::process::Command::new(&self.bin)
            .args(args)
            .output()
            .map_err(|source| BackendError::Herdr {
                message: format!(
                    "failed to spawn herdr binary {}: {source}",
                    self.bin.display()
                ),
            })?;

        if !output.status.success() {
            return Err(BackendError::CommandFailed {
                code: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            });
        }

        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    /// Best-effort lookup of a pane's current shell directory, used to seed the
    /// invocation cwd for path resolution. Returns `None` on any failure (herdr
    /// not running, pane gone, unexpected response) rather than propagating an
    /// error, since callers treat this as an optional hint with a sensible
    /// fallback.
    pub fn query_pane_cwd(&self, pane_id: &str) -> Option<PathBuf> {
        let stdout = self.exec(&pane_get_args(pane_id)).ok()?;
        parse_pane_cwd(&stdout).ok().flatten()
    }
}

impl HerdrBackend for CliBackend {
    fn create_workspace(&mut self, opts: &WorkspaceOpts) -> Result<WorkspaceCreated, BackendError> {
        let stdout = self.exec(&workspace_create_args(opts))?;
        parse_workspace_created(&stdout)
    }

    fn rename_tab(&mut self, tab_id: &str, label: &str) -> Result<(), BackendError> {
        self.exec(&rename_tab_args(tab_id, label))?;
        Ok(())
    }

    fn create_tab(
        &mut self,
        workspace_id: &str,
        opts: &TabOpts,
    ) -> Result<TabCreated, BackendError> {
        let stdout = self.exec(&tab_create_args(workspace_id, opts))?;
        parse_tab_created(&stdout)
    }

    fn split_pane(&mut self, from_pane: &str, opts: &SplitOpts) -> Result<String, BackendError> {
        let stdout = self.exec(&pane_split_args(from_pane, opts))?;
        parse_pane_split(&stdout)
    }

    fn run(&mut self, pane_id: &str, command: &str) -> Result<(), BackendError> {
        self.exec(&pane_run_args(pane_id, command))?;
        Ok(())
    }

    fn wait_output(&mut self, pane_id: &str, wait: &WaitFor) -> Result<(), BackendError> {
        self.exec(&wait_output_args(pane_id, wait))?;
        Ok(())
    }

    fn focus_pane(&mut self, pane_id: &str) -> Result<(), BackendError> {
        self.exec(&focus_args(pane_id))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use super::*;
    use crate::backend::{SplitOpts, TabOpts, WorkspaceOpts};
    use crate::config::{SplitDirection, WaitFor};

    #[test]
    fn should_build_workspace_create_argv_with_cwd_label_env_and_no_focus() {
        let mut env = BTreeMap::new();
        env.insert("FOO".to_string(), "bar".to_string());
        let opts = WorkspaceOpts {
            label: "demo".to_string(),
            cwd: Some(PathBuf::from("/proj")),
            env,
            focus: false,
        };

        let args = workspace_create_args(&opts);

        assert_eq!(
            args,
            vec![
                "workspace",
                "create",
                "--cwd",
                "/proj",
                "--label",
                "demo",
                "--env",
                "FOO=bar",
                "--no-focus"
            ]
        );
    }

    #[test]
    fn should_build_tab_create_argv_with_workspace_cwd_label_and_no_focus() {
        let opts = TabOpts {
            label: Some("editor".to_string()),
            cwd: Some(PathBuf::from("/proj/src")),
            focus: false,
        };

        let args = tab_create_args("w9", &opts);

        assert_eq!(
            args,
            vec![
                "tab",
                "create",
                "--workspace",
                "w9",
                "--cwd",
                "/proj/src",
                "--label",
                "editor",
                "--no-focus"
            ]
        );
    }

    #[test]
    fn should_build_pane_split_argv_with_direction_and_ratio() {
        let opts = SplitOpts {
            direction: SplitDirection::Down,
            ratio: Some(0.3),
            cwd: None,
            env: BTreeMap::new(),
            focus: false,
        };

        let args = pane_split_args("wA:p1", &opts);

        assert_eq!(
            args,
            vec![
                "pane",
                "split",
                "wA:p1",
                "--direction",
                "down",
                "--ratio",
                "0.3",
                "--no-focus"
            ]
        );
    }

    #[test]
    fn should_build_pane_run_argv_with_pane_id_and_command() {
        let args = pane_run_args("wA:p1", "cargo test");

        assert_eq!(args, vec!["pane", "run", "wA:p1", "cargo test"]);
    }

    #[test]
    fn should_build_wait_output_argv_with_regex_free_match_and_timeout() {
        let wait = WaitFor {
            pattern: "Compiling".to_string(),
            timeout_ms: Some(10000),
        };

        let args = wait_output_args("wA:p2", &wait);

        assert_eq!(
            args,
            vec![
                "wait",
                "output",
                "wA:p2",
                "--match",
                "Compiling",
                "--timeout",
                "10000"
            ]
        );
    }

    #[test]
    fn should_build_focus_argv_with_pane_id() {
        let args = focus_args("wA:p1");

        assert_eq!(args, vec!["pane", "focus", "wA:p1"]);
    }

    #[test]
    fn should_extract_nested_ids_when_parsing_workspace_created_response() {
        let json = r#"{"result":{"type":"workspace_created",
          "workspace":{"workspace_id":"wA","label":"__probe_spreader","active_tab_id":"wA:t1"},
          "tab":{"tab_id":"wA:t1","label":"1"},
          "root_pane":{"pane_id":"wA:p1","tab_id":"wA:t1","workspace_id":"wA","cwd":"/tmp"}}}"#;

        let created = parse_workspace_created(json).unwrap();

        assert_eq!(created.workspace_id, "wA");
        assert_eq!(created.tab_id, "wA:t1");
        assert_eq!(created.root_pane_id, "wA:p1");
    }

    #[test]
    fn should_extract_tab_and_root_pane_ids_when_parsing_tab_created_response() {
        let json = r#"{"result":{"type":"tab_created","tab":{"tab_id":"wA:t2"},"root_pane":{"pane_id":"wA:p2"}}}"#;

        let created = parse_tab_created(json).unwrap();

        assert_eq!(created.tab_id, "wA:t2");
        assert_eq!(created.root_pane_id, "wA:p2");
    }

    #[test]
    fn should_extract_new_pane_id_when_parsing_pane_info_response() {
        let json = r#"{"result":{"type":"pane_info","pane":{"pane_id":"wA:p3","tab_id":"wA:t1"}}}"#;

        let pane_id = parse_pane_split(json).unwrap();

        assert_eq!(pane_id, "wA:p3");
    }

    #[test]
    fn should_build_pane_get_argv_with_pane_id() {
        let args = pane_get_args("wA:p1");

        assert_eq!(args, vec!["pane", "get", "wA:p1"]);
    }

    #[test]
    fn should_extract_foreground_cwd_when_parsing_pane_get_response() {
        let json = r#"{"result":{"type":"pane_info","pane":{"pane_id":"wA:p1","tab_id":"wA:t1","foreground_cwd":"/home/demo/project"}}}"#;

        let cwd = parse_pane_cwd(json).unwrap();

        assert_eq!(cwd, Some(PathBuf::from("/home/demo/project")));
    }

    #[test]
    fn should_return_none_cwd_when_pane_get_response_omits_foreground_cwd() {
        let json = r#"{"result":{"type":"pane_info","pane":{"pane_id":"wA:p1","tab_id":"wA:t1"}}}"#;

        let cwd = parse_pane_cwd(json).unwrap();

        assert_eq!(cwd, None);
    }

    #[test]
    fn should_return_none_when_querying_pane_cwd_against_a_binary_that_cannot_be_spawned() {
        let backend = CliBackend::new(PathBuf::from("/no/such/herdr-binary-xyz"));

        let cwd = backend.query_pane_cwd("wA:p1");

        assert_eq!(cwd, None);
    }

    #[test]
    fn should_return_backend_error_containing_message_when_response_is_error_json() {
        let json = r#"{"id":"abc","error":{"code":"invalid_request","message":"unknown variant `bogus`"}}"#;

        let result = parse_workspace_created(json);

        let err = result.unwrap_err();
        match err {
            BackendError::Herdr { message } => {
                assert!(message.contains("unknown variant"));
            }
            other => panic!("expected BackendError::Herdr, got {other:?}"),
        }
    }

    #[test]
    fn should_return_command_failed_error_with_stderr_when_process_exits_non_zero() {
        let backend = CliBackend::new(PathBuf::from("/bin/sh"));

        let result = backend.exec(&["-c".to_string(), "echo boom >&2; exit 7".to_string()]);

        let err = result.unwrap_err();
        match err {
            BackendError::CommandFailed { code, stderr } => {
                assert_eq!(code, Some(7));
                assert!(stderr.contains("boom"));
            }
            other => panic!("expected BackendError::CommandFailed, got {other:?}"),
        }
    }

    #[test]
    fn should_return_herdr_error_when_binary_cannot_be_spawned() {
        let backend = CliBackend::new(PathBuf::from("/no/such/herdr-binary-xyz"));

        let result = backend.exec(&["workspace".to_string(), "create".to_string()]);

        assert!(result.is_err());
    }

    #[test]
    fn should_build_rename_tab_argv_with_tab_id_and_label() {
        let args = rename_tab_args("wA:t1", "editor");

        assert_eq!(args, vec!["tab", "rename", "wA:t1", "editor"]);
    }

    #[test]
    fn should_resolve_herdr_binary_from_env_and_fall_back_to_herdr_on_path() {
        let mut env_with_bin = BTreeMap::new();
        env_with_bin.insert("HERDR_BIN_PATH".to_string(), "/opt/herdr".to_string());

        let resolved = CliBackend::resolve_bin(&env_with_bin);
        assert_eq!(resolved, PathBuf::from("/opt/herdr"));

        let env_without_bin = BTreeMap::new();
        let fallback = CliBackend::resolve_bin(&env_without_bin);
        assert_eq!(fallback, PathBuf::from("herdr"));
    }
}
