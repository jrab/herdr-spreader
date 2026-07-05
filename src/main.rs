use std::collections::BTreeMap;

use clap::Parser;

use herdr_spreader::backend::cli::CliBackend;
use herdr_spreader::cli::{Cli, Command};
use herdr_spreader::config::{load_config, resolve_config_path, resolve_paths};
use herdr_spreader::engine;

fn main() -> anyhow::Result<()> {
    let env: BTreeMap<String, String> = std::env::vars().collect();
    let cli = Cli::parse();

    let Command::Apply { file } = cli.command;

    let config_path = resolve_config_path(file, &env)?;
    let spread_file = load_config(&config_path)?;

    let bin = CliBackend::resolve_bin(&env);
    let mut backend = CliBackend::new(bin);

    // Under `herdr plugin action invoke`, the process cwd is the plugin's own
    // install directory, not the user's workspace. Query the invoking pane's
    // real shell directory over the herdr CLI instead; fall back to the
    // process cwd for standalone CLI usage (no herdr session, or the query
    // fails) where the process cwd is the correct invocation directory.
    let cwd = match env
        .get("HERDR_PANE_ID")
        .and_then(|id| backend.query_pane_cwd(id))
    {
        Some(cwd) => cwd,
        None => std::env::current_dir()?,
    };
    let spread_file = resolve_paths(spread_file, &env, &cwd);

    engine::apply(&spread_file, &mut backend)?;

    Ok(())
}
