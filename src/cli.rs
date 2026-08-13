use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "herdr-spreader",
    about = "Apply tmuxinator-style project layouts from YAML"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand, PartialEq)]
pub enum Command {
    Apply {
        #[arg(long, short)]
        file: Option<PathBuf>,
        #[arg(long)]
        dry_run: bool,
    },
    ApplyExisting {
        #[arg(long, short)]
        file: Option<PathBuf>,
        #[arg(long)]
        workspace_id: String,
        #[arg(long)]
        tab_id: String,
        #[arg(long)]
        root_pane_id: String,
        /// Definitive root for relative layout paths. Prefer this when the
        /// existing pane may still be running shell startup hooks.
        #[arg(long)]
        root: Option<PathBuf>,
        #[arg(long)]
        dry_run: bool,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_parse_apply_subcommand_with_optional_config_file_argument() {
        let cli =
            Cli::try_parse_from(["herdr-spreader", "apply", "--file", "./spread.yml"]).unwrap();
        assert_eq!(
            cli.command,
            Command::Apply {
                file: Some(PathBuf::from("./spread.yml")),
                dry_run: false,
            }
        );

        let cli = Cli::try_parse_from(["herdr-spreader", "apply"]).unwrap();
        assert_eq!(
            cli.command,
            Command::Apply {
                file: None,
                dry_run: false
            }
        );

        let cli = Cli::try_parse_from(["herdr-spreader", "apply", "-f", "./spread.yml"]).unwrap();
        assert_eq!(
            cli.command,
            Command::Apply {
                file: Some(PathBuf::from("./spread.yml")),
                dry_run: false,
            }
        );
    }

    #[test]
    fn should_parse_apply_subcommand_with_dry_run_flag() {
        let cli = Cli::try_parse_from(["herdr-spreader", "apply", "--dry-run"]).unwrap();
        assert_eq!(
            cli.command,
            Command::Apply {
                file: None,
                dry_run: true
            }
        );
        let cli =
            Cli::try_parse_from(["herdr-spreader", "apply", "--file", "x", "--dry-run"]).unwrap();
        assert_eq!(
            cli.command,
            Command::Apply {
                file: Some(PathBuf::from("x")),
                dry_run: true
            }
        );
        let cli = Cli::try_parse_from(["herdr-spreader", "apply"]).unwrap();
        assert_eq!(
            cli.command,
            Command::Apply {
                file: None,
                dry_run: false
            }
        );
    }

    #[test]
    fn should_reject_validate_subcommand() {
        let result = Cli::try_parse_from(["herdr-spreader", "validate"]);
        assert!(
            result.is_err(),
            "validate subcommand should no longer be parsed"
        );
    }

    #[test]
    fn should_parse_apply_existing_with_required_target_ids() {
        let cli = Cli::try_parse_from([
            "herdr-spreader",
            "apply-existing",
            "--file",
            "./spread.yml",
            "--workspace-id",
            "w2",
            "--tab-id",
            "w2:t1",
            "--root-pane-id",
            "w2:p1",
        ])
        .unwrap();

        assert_eq!(
            cli.command,
            Command::ApplyExisting {
                file: Some(PathBuf::from("./spread.yml")),
                workspace_id: "w2".into(),
                tab_id: "w2:t1".into(),
                root_pane_id: "w2:p1".into(),
                root: None,
                dry_run: false,
            }
        );
    }

    #[test]
    fn should_parse_apply_existing_with_explicit_root() {
        let cli = Cli::try_parse_from([
            "herdr-spreader",
            "apply-existing",
            "--workspace-id",
            "w2",
            "--tab-id",
            "w2:t1",
            "--root-pane-id",
            "w2:p1",
            "--root",
            "/worktrees/topic",
        ])
        .unwrap();

        assert_eq!(
            cli.command,
            Command::ApplyExisting {
                file: None,
                workspace_id: "w2".into(),
                tab_id: "w2:t1".into(),
                root_pane_id: "w2:p1".into(),
                root: Some(PathBuf::from("/worktrees/topic")),
                dry_run: false,
            }
        );
    }
}
