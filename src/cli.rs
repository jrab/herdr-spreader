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
}
