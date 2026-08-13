use std::collections::BTreeMap;
use std::path::PathBuf;

use clap::Parser;

use herdr_spreader::backend::cli::CliBackend;
use herdr_spreader::cli::{Cli, Command};
use herdr_spreader::config::{read_config, resolve_config_path, resolve_paths};
use herdr_spreader::engine;
use herdr_spreader::validate;

fn main() -> anyhow::Result<()> {
    let env: BTreeMap<String, String> = std::env::vars().collect();
    let cli = Cli::parse();

    match cli.command {
        Command::Apply { file, dry_run } => {
            let config_path = resolve_config_path(file, &env)?;
            let contents = read_config(&config_path)?;
            let spread_file = match validate::validate_config(validate::SourceFile {
                yaml: &contents,
                path: &config_path,
            }) {
                Ok(f) => f,
                Err(findings) => {
                    validate::print_findings(&findings);
                    std::process::exit(1);
                }
            };

            let bin = CliBackend::resolve_bin(&env);
            let socket_path = env.get("HERDR_SOCKET_PATH").map(PathBuf::from);
            let mut backend = CliBackend::new(bin, socket_path);

            let cwd = match env
                .get("HERDR_PANE_ID")
                .and_then(|id| backend.query_pane_cwd(id))
            {
                Some(cwd) => cwd,
                None => std::env::current_dir()?,
            };
            let spread_file = resolve_paths(&spread_file, &env, &cwd);

            if dry_run {
                let plan = engine::plan_file(&spread_file);
                for op in &plan {
                    println!("{}", engine::render_op(op));
                }
                return Ok(());
            }

            engine::apply(&spread_file, &mut backend)?;

            Ok(())
        }
        Command::ApplyExisting {
            file,
            workspace_id,
            tab_id,
            root_pane_id,
            root,
            dry_run,
        } => {
            let config_path = resolve_config_path(file, &env)?;
            let contents = read_config(&config_path)?;
            let spread_file = match validate::validate_config(validate::SourceFile {
                yaml: &contents,
                path: &config_path,
            }) {
                Ok(f) => f,
                Err(findings) => {
                    validate::print_findings(&findings);
                    std::process::exit(1);
                }
            };
            let [workspace] = spread_file.workspaces.as_slice() else {
                anyhow::bail!("apply-existing requires a config containing exactly one workspace");
            };

            let bin = CliBackend::resolve_bin(&env);
            let socket_path = env.get("HERDR_SOCKET_PATH").map(PathBuf::from);
            let mut backend = CliBackend::new(bin, socket_path);
            // A freshly created pane can briefly report a directory visited by
            // shell startup hooks (for example ~/.oh-my-zsh). Callers that
            // created the workspace already know its authoritative root, so
            // let them bypass that transient pane state.
            let cwd = match root {
                Some(root) if root.is_absolute() => root,
                Some(root) => anyhow::bail!(
                    "apply-existing --root must be an absolute path: {}",
                    root.display()
                ),
                None => backend
                    .query_pane_cwd(&root_pane_id)
                    .unwrap_or(std::env::current_dir()?),
            };
            let resolved = resolve_paths(
                &herdr_spreader::config::SpreadFile {
                    workspaces: vec![workspace.clone()],
                },
                &env,
                &cwd,
            );
            let workspace = &resolved.workspaces[0];
            let target = engine::ExistingWorkspace {
                workspace_id,
                tab_id,
                root_pane_id,
            };

            if dry_run {
                for op in engine::plan_existing_workspace(workspace) {
                    println!("{}", engine::render_op(&op));
                }
                return Ok(());
            }

            engine::apply_to_existing(workspace, &target, &mut backend)?;
            Ok(())
        }
    }
}
