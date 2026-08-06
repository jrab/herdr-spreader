use std::path::Path;

use herdr_spreader::config::SpreadFile;
use herdr_spreader::engine::{BackendOp, PaneHandle, plan_file};
use herdr_spreader::validate::{SourceFile, validate_config};

const GRID: &str = include_str!("../examples/grid.yaml");

#[test]
fn should_plan_balanced_two_by_two_grid_by_splitting_both_columns() {
    let file = SpreadFile::from_str(GRID).unwrap();
    let split_pairs: Vec<_> = plan_file(&file)
        .into_iter()
        .filter_map(|op| match op {
            BackendOp::SplitPane { from, into, .. } => Some((from, into)),
            _ => None,
        })
        .collect();

    assert_eq!(
        split_pairs,
        vec![
            (PaneHandle::TabRoot(0), PaneHandle::Split(1)),
            (PaneHandle::TabRoot(0), PaneHandle::Split(2)),
            (PaneHandle::Split(1), PaneHandle::Split(3)),
        ]
    );
}

#[test]
fn should_validate_bundled_grid_layout() {
    assert!(
        validate_config(SourceFile {
            yaml: GRID,
            path: Path::new("examples/grid.yaml"),
        })
        .is_ok()
    );
}

#[test]
fn should_reject_layout_split_without_exactly_two_children() {
    let yaml = r"
workspaces:
  - name: broken
    tabs:
      - layout:
          split: right
          children:
            - pane: {}
";
    let findings = validate_config(SourceFile {
        yaml,
        path: Path::new("broken.yaml"),
    })
    .unwrap_err();

    assert!(
        findings
            .iter()
            .any(|finding| finding.message.contains("exactly 2 children"))
    );
}

#[test]
fn should_reject_tab_that_combines_legacy_panes_and_layout_tree() {
    let yaml = r"
workspaces:
  - name: broken
    tabs:
      - panes:
          - command: nvim
        layout:
          pane: {}
";
    let findings = validate_config(SourceFile {
        yaml,
        path: Path::new("broken.yaml"),
    })
    .unwrap_err();

    assert!(
        findings
            .iter()
            .any(|finding| finding.message.contains("either `panes` or `layout`"))
    );
}
