use std::collections::BTreeMap;
use std::path::PathBuf;

use clap::Parser;

use herdr_spreader::backend::HerdrBackend;
use herdr_spreader::backend::cli::CliBackend;
use herdr_spreader::cli::{Cli, Command};
use herdr_spreader::config::{read_config, resolve_config_path, resolve_paths};
use herdr_spreader::engine;
use herdr_spreader::validate;

#[derive(Clone, Copy)]
enum ExistingApplyMode {
    KeepCurrentTab,
    ReflowCurrentTab,
}

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
        } => apply_existing(
            file,
            &engine::ExistingWorkspace {
                workspace_id,
                tab_id,
                root_pane_id,
            },
            root,
            dry_run,
            &env,
            ExistingApplyMode::KeepCurrentTab,
        ),
        Command::ApplyCurrent { file, dry_run } => apply_existing(
            file,
            &engine::ExistingWorkspace {
                workspace_id: plugin_context_id(&env, "HERDR_WORKSPACE_ID")?,
                tab_id: plugin_context_id(&env, "HERDR_TAB_ID")?,
                root_pane_id: plugin_context_id(&env, "HERDR_PANE_ID")?,
            },
            None,
            dry_run,
            &env,
            ExistingApplyMode::ReflowCurrentTab,
        ),
    }
}

fn plugin_context_id(env: &BTreeMap<String, String>, name: &str) -> anyhow::Result<String> {
    env.get(name)
        .filter(|value| !value.is_empty())
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("apply-current requires {name} from Herdr's plugin context"))
}

fn is_transient_palette(label: Option<&str>) -> bool {
    matches!(label, Some("Command palette" | "Palette"))
}

fn without_transient_palette(
    target: &engine::ExistingWorkspace,
    panes: Vec<herdr_spreader::backend::PaneInfo>,
    invoking_pane_label: Option<&str>,
) -> anyhow::Result<(
    engine::ExistingWorkspace,
    Vec<herdr_spreader::backend::PaneInfo>,
)> {
    if !is_transient_palette(invoking_pane_label) {
        return Ok((target.clone(), panes));
    }

    let durable_panes: Vec<_> = panes
        .into_iter()
        .filter(|pane| pane.pane_id != target.root_pane_id)
        .collect();
    let root_pane_id = durable_panes
        .iter()
        .find(|pane| pane.tab_id == target.tab_id)
        .map(|pane| pane.pane_id.clone())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "apply-current could not find a durable pane behind the temporary palette"
            )
        })?;

    Ok((
        engine::ExistingWorkspace {
            workspace_id: target.workspace_id.clone(),
            tab_id: target.tab_id.clone(),
            root_pane_id,
        },
        durable_panes,
    ))
}

fn apply_existing(
    file: Option<PathBuf>,
    target: &engine::ExistingWorkspace,
    root: Option<PathBuf>,
    dry_run: bool,
    env: &BTreeMap<String, String>,
    mode: ExistingApplyMode,
) -> anyhow::Result<()> {
    let config_path = resolve_config_path(file, env)?;
    let contents = read_config(&config_path)?;
    let spread_file = match validate::validate_config(validate::SourceFile {
        yaml: &contents,
        path: &config_path,
    }) {
        Ok(file) => file,
        Err(findings) => {
            validate::print_findings(&findings);
            std::process::exit(1);
        }
    };
    let [workspace] = spread_file.workspaces.as_slice() else {
        anyhow::bail!("apply-existing requires a config containing exactly one workspace");
    };

    let bin = CliBackend::resolve_bin(env);
    let socket_path = env.get("HERDR_SOCKET_PATH").map(PathBuf::from);
    let mut backend = CliBackend::new(bin, socket_path);
    // A freshly created pane can briefly report a directory visited by shell
    // startup hooks. Callers that created the workspace already know its
    // authoritative root, so let them bypass that transient pane state.
    let cwd = match root {
        Some(root) if root.is_absolute() => root,
        Some(root) => anyhow::bail!(
            "apply-existing --root must be an absolute path: {}",
            root.display()
        ),
        None => backend
            .query_pane_cwd(&target.root_pane_id)
            .unwrap_or(std::env::current_dir()?),
    };
    let resolved = resolve_paths(
        &herdr_spreader::config::SpreadFile {
            workspaces: vec![workspace.clone()],
        },
        env,
        &cwd,
    );
    let workspace = &resolved.workspaces[0];

    if dry_run {
        for op in engine::plan_existing_workspace(workspace) {
            println!("{}", engine::render_op(&op));
        }
        return Ok(());
    }

    match mode {
        ExistingApplyMode::KeepCurrentTab => {
            engine::apply_to_existing(workspace, target, &mut backend)?;
        }
        ExistingApplyMode::ReflowCurrentTab => {
            let panes = backend.list_panes(&target.workspace_id)?;
            let invoking_pane_label = backend.query_pane_label(&target.root_pane_id);
            let (target, panes) =
                without_transient_palette(target, panes, invoking_pane_label.as_deref())?;
            engine::apply_to_current(workspace, &target, &panes, &mut backend)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_read_non_empty_plugin_context_id() {
        let env = BTreeMap::from([("HERDR_WORKSPACE_ID".into(), "w7".into())]);

        assert_eq!(plugin_context_id(&env, "HERDR_WORKSPACE_ID").unwrap(), "w7");
    }

    #[test]
    fn should_reject_missing_or_empty_plugin_context_id() {
        assert!(plugin_context_id(&BTreeMap::new(), "HERDR_WORKSPACE_ID").is_err());

        let env = BTreeMap::from([("HERDR_WORKSPACE_ID".into(), String::new())]);
        assert!(plugin_context_id(&env, "HERDR_WORKSPACE_ID").is_err());
    }
}
