use std::collections::BTreeMap;
use std::path::PathBuf;

use thiserror::Error;

use crate::config::{SplitDirection, WaitFor};

pub mod cli;

#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceOpts {
    pub label: String,
    pub cwd: Option<PathBuf>,
    pub env: BTreeMap<String, String>,
    pub focus: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceCreated {
    pub workspace_id: String,
    pub tab_id: String,
    pub root_pane_id: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TabOpts {
    pub label: Option<String>,
    pub cwd: Option<PathBuf>,
    pub focus: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TabCreated {
    pub tab_id: String,
    pub root_pane_id: String,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct SplitOpts {
    pub direction: SplitDirection,
    pub ratio: Option<f64>,
    pub cwd: Option<PathBuf>,
    pub env: BTreeMap<String, String>,
    pub focus: bool,
}

#[derive(Debug, Error)]
pub enum BackendError {
    #[error("backend error: {message}")]
    Herdr { message: String },
    #[error("herdr command failed (exit code {code:?}): {stderr}")]
    CommandFailed { code: Option<i32>, stderr: String },
}

pub trait HerdrBackend {
    fn create_workspace(&mut self, opts: &WorkspaceOpts) -> Result<WorkspaceCreated, BackendError>;
    fn rename_tab(&mut self, tab_id: &str, label: &str) -> Result<(), BackendError>;
    fn create_tab(
        &mut self,
        workspace_id: &str,
        opts: &TabOpts,
    ) -> Result<TabCreated, BackendError>;
    fn split_pane(&mut self, from_pane: &str, opts: &SplitOpts) -> Result<String, BackendError>;
    fn run(&mut self, pane_id: &str, command: &str) -> Result<(), BackendError>;
    fn wait_output(&mut self, pane_id: &str, wait: &WaitFor) -> Result<(), BackendError>;
    fn focus_pane(&mut self, pane_id: &str) -> Result<(), BackendError>;
}
