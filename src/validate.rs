use std::collections::HashSet;
use std::path::Path;

use crate::config::{LayoutNode, Pane, SpreadFile};

#[derive(Debug, Clone, PartialEq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ValidationFinding {
    pub severity: Severity,
    pub message: String,
}

#[derive(Debug, Clone, Copy)]
pub struct SourceFile<'a> {
    pub yaml: &'a str,
    pub path: &'a Path,
}

/// Validate a config file's contents: parse the YAML, then run semantic checks.
///
/// # Errors
///
/// Returns a `Vec<ValidationFinding>` if the YAML fails to parse (a single
/// `Severity::Error` finding naming `source.path`) or if any semantic finding
/// (error *or* warning) is reported; otherwise returns the parsed [`SpreadFile`].
pub fn validate_config(source: SourceFile<'_>) -> Result<SpreadFile, Vec<ValidationFinding>> {
    let file = SpreadFile::from_str(source.yaml).map_err(|err| {
        vec![ValidationFinding {
            severity: Severity::Error,
            message: format!(
                "failed to parse config file {}: {err}",
                source.path.display()
            ),
        }]
    })?;
    let findings = validate(&file);
    if findings.is_empty() {
        Ok(file)
    } else {
        Err(findings)
    }
}

pub(crate) fn validate(file: &SpreadFile) -> Vec<ValidationFinding> {
    let mut findings = Vec::new();

    let mut seen_names: HashSet<&str> = HashSet::new();
    let mut focus_count = 0usize;

    for ws in &file.workspaces {
        if ws.name.is_empty() {
            findings.push(ValidationFinding {
                severity: Severity::Error,
                message: "workspace name must not be empty".to_string(),
            });
        }
        if !seen_names.insert(ws.name.as_str()) {
            findings.push(ValidationFinding {
                severity: Severity::Error,
                message: format!("duplicate workspace name '{}'", ws.name),
            });
        }
        if ws.focus {
            focus_count += 1;
        }

        for tab in &ws.tabs {
            if !tab.panes.is_empty() && tab.layout.is_some() {
                findings.push(ValidationFinding {
                    severity: Severity::Error,
                    message: format!(
                        "tab '{}' in workspace '{}' must use either `panes` or `layout`, not both",
                        tab.label.as_deref().unwrap_or("(unnamed)"),
                        ws.name
                    ),
                });
            }
            if tab.panes.is_empty() && tab.layout.is_none() {
                findings.push(ValidationFinding {
                    severity: Severity::Warning,
                    message: format!(
                        "tab '{}' in workspace '{}' has no panes",
                        tab.label.as_deref().unwrap_or("(unnamed)"),
                        ws.name
                    ),
                });
            }

            for pane in &tab.panes {
                validate_pane(pane, &ws.name, &mut findings);
            }
            if let Some(layout) = &tab.layout {
                validate_layout(layout, &ws.name, &mut findings);
            }
        }
    }

    if focus_count > 1 {
        findings.push(ValidationFinding {
            severity: Severity::Warning,
            message: format!(
                "{focus_count} workspaces have `focus: true`; only the last one will receive focus"
            ),
        });
    }

    findings
}

fn validate_ratio(ratio: Option<f64>, workspace: &str, findings: &mut Vec<ValidationFinding>) {
    if let Some(ratio) = ratio
        && (ratio <= 0.0 || ratio >= 1.0)
    {
        findings.push(ValidationFinding {
            severity: Severity::Error,
            message: format!(
                "pane ratio {ratio} in workspace '{workspace}' must be between 0 and 1 (exclusive)"
            ),
        });
    }
}

fn validate_pane(pane: &Pane, workspace: &str, findings: &mut Vec<ValidationFinding>) {
    validate_ratio(pane.ratio, workspace, findings);
    if pane.wait_for.is_some() && pane.command.is_none() {
        findings.push(ValidationFinding {
            severity: Severity::Error,
            message: format!(
                "wait_for was specified on a pane without a command in workspace '{workspace}'"
            ),
        });
    }
}

fn validate_layout(layout: &LayoutNode, workspace: &str, findings: &mut Vec<ValidationFinding>) {
    match layout {
        LayoutNode::Pane(leaf) => validate_pane(&leaf.pane, workspace, findings),
        LayoutNode::Split(node) => {
            validate_ratio(node.ratio, workspace, findings);
            if node.children.len() != 2 {
                findings.push(ValidationFinding {
                    severity: Severity::Error,
                    message: format!(
                        "layout split in workspace '{workspace}' must have exactly 2 children"
                    ),
                });
            }
            for child in &node.children {
                validate_layout(child, workspace, findings);
            }
        }
    }
}

pub fn print_findings(findings: &[ValidationFinding]) {
    for finding in findings {
        let tag = match finding.severity {
            Severity::Error => "ERROR",
            Severity::Warning => "WARNING",
        };
        eprintln!("[{tag}] {}", finding.message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Pane, SpreadFile, Tab, WaitFor, Workspace};

    #[test]
    fn should_return_empty_findings_list_given_valid_minimal_config() {
        let file = SpreadFile {
            workspaces: vec![Workspace {
                name: "demo".to_string(),
                ..Default::default()
            }],
        };
        let findings = validate(&file);
        assert!(findings.is_empty());
    }

    #[test]
    fn should_report_error_given_workspace_with_empty_name() {
        let file = SpreadFile {
            workspaces: vec![Workspace {
                name: String::new(),
                ..Default::default()
            }],
        };
        let findings = validate(&file);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Error);
        assert!(findings[0].message.contains("empty"));
    }

    #[test]
    fn should_report_error_given_duplicate_workspace_names() {
        let file = SpreadFile {
            workspaces: vec![
                Workspace {
                    name: "frontend".to_string(),
                    ..Default::default()
                },
                Workspace {
                    name: "frontend".to_string(),
                    ..Default::default()
                },
            ],
        };
        let findings = validate(&file);
        let dup_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.message.contains("duplicate"))
            .collect();
        assert_eq!(dup_findings.len(), 1);
        assert_eq!(dup_findings[0].severity, Severity::Error);
    }

    #[test]
    fn should_report_error_given_ratio_equal_to_zero() {
        let file = SpreadFile {
            workspaces: vec![Workspace {
                name: "demo".to_string(),
                tabs: vec![Tab {
                    panes: vec![Pane {
                        ratio: Some(0.0),
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            }],
        };
        let findings = validate(&file);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Error);
    }

    #[test]
    fn should_report_error_given_ratio_equal_to_one() {
        let file = SpreadFile {
            workspaces: vec![Workspace {
                name: "demo".to_string(),
                tabs: vec![Tab {
                    panes: vec![Pane {
                        ratio: Some(1.0),
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            }],
        };
        let findings = validate(&file);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Error);
    }

    #[test]
    fn should_report_error_given_negative_ratio() {
        let file = SpreadFile {
            workspaces: vec![Workspace {
                name: "demo".to_string(),
                tabs: vec![Tab {
                    panes: vec![Pane {
                        ratio: Some(-0.5),
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            }],
        };
        let findings = validate(&file);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Error);
    }

    #[test]
    fn should_skip_ratio_check_when_ratio_is_none() {
        let file = SpreadFile {
            workspaces: vec![Workspace {
                name: "demo".to_string(),
                tabs: vec![Tab {
                    panes: vec![Pane {
                        ratio: None,
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            }],
        };
        let findings = validate(&file);
        assert!(findings.is_empty());
    }

    #[test]
    fn should_accept_valid_ratio_between_zero_and_one() {
        let file = SpreadFile {
            workspaces: vec![Workspace {
                name: "demo".to_string(),
                tabs: vec![Tab {
                    panes: vec![Pane {
                        ratio: Some(0.5),
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            }],
        };
        let findings = validate(&file);
        assert!(findings.is_empty());
    }

    #[test]
    fn should_report_warning_given_multiple_focused_workspaces() {
        let file = SpreadFile {
            workspaces: vec![
                Workspace {
                    name: "alpha".to_string(),
                    focus: true,
                    ..Default::default()
                },
                Workspace {
                    name: "beta".to_string(),
                    focus: false,
                    ..Default::default()
                },
                Workspace {
                    name: "gamma".to_string(),
                    focus: true,
                    ..Default::default()
                },
            ],
        };
        let findings = validate(&file);
        let focus_warnings: Vec<_> = findings
            .iter()
            .filter(|f| f.severity == Severity::Warning && f.message.contains("focus"))
            .collect();
        assert_eq!(focus_warnings.len(), 1);
    }

    #[test]
    fn should_not_report_warning_given_exactly_one_focused_workspace() {
        let file = SpreadFile {
            workspaces: vec![
                Workspace {
                    name: "alpha".to_string(),
                    focus: true,
                    ..Default::default()
                },
                Workspace {
                    name: "beta".to_string(),
                    ..Default::default()
                },
            ],
        };
        let findings = validate(&file);
        let focus_warnings: Vec<_> = findings
            .iter()
            .filter(|f| f.message.contains("focus"))
            .collect();
        assert!(focus_warnings.is_empty());
    }

    #[test]
    fn should_not_report_warning_given_zero_focused_workspaces() {
        let file = SpreadFile {
            workspaces: vec![
                Workspace {
                    name: "alpha".to_string(),
                    ..Default::default()
                },
                Workspace {
                    name: "beta".to_string(),
                    ..Default::default()
                },
            ],
        };
        let findings = validate(&file);
        let focus_warnings: Vec<_> = findings
            .iter()
            .filter(|f| f.message.contains("focus"))
            .collect();
        assert!(focus_warnings.is_empty());
    }

    #[test]
    fn should_report_warning_given_tab_with_no_panes() {
        let file = SpreadFile {
            workspaces: vec![Workspace {
                name: "demo".to_string(),
                tabs: vec![Tab {
                    label: None,
                    panes: vec![],
                    ..Default::default()
                }],
                ..Default::default()
            }],
        };
        let findings = validate(&file);
        let tab_warnings: Vec<_> = findings
            .iter()
            .filter(|f| f.severity == Severity::Warning && f.message.contains("no panes"))
            .collect();
        assert_eq!(tab_warnings.len(), 1);
    }

    #[test]
    fn should_not_report_warning_given_tab_with_panes() {
        let file = SpreadFile {
            workspaces: vec![Workspace {
                name: "demo".to_string(),
                tabs: vec![Tab {
                    label: None,
                    panes: vec![Pane {
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            }],
        };
        let findings = validate(&file);
        let tab_warnings: Vec<_> = findings
            .iter()
            .filter(|f| f.message.contains("no panes"))
            .collect();
        assert!(tab_warnings.is_empty());
    }

    #[test]
    fn should_report_error_given_pane_with_wait_for_but_no_command() {
        let file = SpreadFile {
            workspaces: vec![Workspace {
                name: "demo".to_string(),
                tabs: vec![Tab {
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
            }],
        };
        let findings = validate(&file);
        let wait_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.message.contains("wait_for"))
            .collect();
        assert_eq!(wait_findings.len(), 1);
        assert_eq!(wait_findings[0].severity, Severity::Error);
    }

    #[test]
    fn should_not_report_wait_for_error_given_pane_with_both_command_and_wait_for() {
        let file = SpreadFile {
            workspaces: vec![Workspace {
                name: "demo".to_string(),
                tabs: vec![Tab {
                    panes: vec![Pane {
                        command: Some("cargo run".to_string()),
                        wait_for: Some(WaitFor {
                            pattern: "ready".to_string(),
                            timeout_ms: None,
                        }),
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            }],
        };
        let findings = validate(&file);
        let wait_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.message.contains("wait_for"))
            .collect();
        assert!(wait_findings.is_empty());
    }

    #[test]
    fn should_return_ok_with_spread_file_given_clean_yaml() {
        let source = SourceFile {
            yaml: "workspaces:\n  - name: demo\n",
            path: Path::new("config.yaml"),
        };
        let result = validate_config(source);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().workspaces[0].name, "demo");
    }

    #[test]
    fn should_return_err_with_parse_finding_given_malformed_yaml() {
        let source = SourceFile {
            yaml: "name: demo\ntabs: []",
            path: Path::new("/cfg/spread.yaml"),
        };
        let result = validate_config(source);
        let err = result.unwrap_err();
        assert_eq!(err.len(), 1);
        assert_eq!(err[0].severity, Severity::Error);
        assert!(err[0].message.contains("/cfg/spread.yaml"));
        assert!(err[0].message.to_lowercase().contains("parse"));
    }

    #[test]
    fn should_return_err_with_findings_given_duplicate_workspace_names() {
        let source = SourceFile {
            yaml: "workspaces:\n  - name: frontend\n  - name: frontend\n",
            path: Path::new("c.yaml"),
        };
        let result = validate_config(source);
        let err = result.unwrap_err();
        assert!(
            err.iter()
                .any(|f| f.message.contains("duplicate") && f.severity == Severity::Error)
        );
    }

    #[test]
    fn should_return_err_given_warning_only_config_with_empty_tab() {
        let source = SourceFile {
            yaml: "workspaces:\n  - name: demo\n    tabs:\n      - panes: []\n",
            path: Path::new("c.yaml"),
        };
        let result = validate_config(source);
        let err = result.unwrap_err();
        assert!(
            err.iter()
                .any(|f| f.severity == Severity::Warning && f.message.contains("no panes"))
        );
    }

    #[test]
    fn should_pass_validation_given_bundled_example_config() {
        let source = SourceFile {
            yaml: include_str!("../examples/config.yaml"),
            path: Path::new("examples/config.yaml"),
        };
        assert!(validate_config(source).is_ok());
    }
}
