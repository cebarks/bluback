use std::io;

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;

use crate::history_cli;
use crate::Args;

#[derive(Parser, Debug)]
#[command(
    name = "bluback generate",
    about = "Generate shell completions and man pages"
)]
pub struct GenerateArgs {
    #[command(subcommand)]
    pub command: GenerateCommand,
}

#[derive(Subcommand, Debug)]
pub enum GenerateCommand {
    /// Generate shell completion script
    Completions {
        /// Target shell
        shell: Shell,
    },
    /// Generate man page
    Man,
}

/// Build the full CLI command tree including pre-dispatched subcommands
/// (history, generate) so completions and man pages cover the entire CLI.
pub fn full_command() -> clap::Command {
    Args::command()
        .subcommand(history_cli::HistoryArgs::command().name("history"))
        .subcommand(GenerateArgs::command().name("generate"))
}

pub fn run_generate(args: GenerateArgs) -> anyhow::Result<()> {
    match args.command {
        GenerateCommand::Completions { shell } => {
            let mut cmd = full_command();
            clap_complete::generate(shell, &mut cmd, "bluback", &mut io::stdout());
        }
        GenerateCommand::Man => {
            let cmd = full_command();
            let man = clap_mangen::Man::new(cmd);
            man.render(&mut io::stdout())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_command_includes_subcommands() {
        let cmd = full_command();
        let sub_names: Vec<&str> = cmd.get_subcommands().map(|s| s.get_name()).collect();
        assert!(sub_names.contains(&"history"), "missing history subcommand");
        assert!(
            sub_names.contains(&"generate"),
            "missing generate subcommand"
        );
    }

    #[test]
    fn full_command_history_has_list() {
        let cmd = full_command();
        let history = cmd
            .get_subcommands()
            .find(|s| s.get_name() == "history")
            .expect("history subcommand exists");
        let sub_names: Vec<&str> = history.get_subcommands().map(|s| s.get_name()).collect();
        assert!(
            sub_names.contains(&"list"),
            "history missing list subcommand"
        );
        assert!(
            sub_names.contains(&"show"),
            "history missing show subcommand"
        );
        assert!(
            sub_names.contains(&"stats"),
            "history missing stats subcommand"
        );
    }

    #[test]
    fn full_command_generate_has_completions_and_man() {
        let cmd = full_command();
        let generate = cmd
            .get_subcommands()
            .find(|s| s.get_name() == "generate")
            .expect("generate subcommand exists");
        let sub_names: Vec<&str> = generate.get_subcommands().map(|s| s.get_name()).collect();
        assert!(
            sub_names.contains(&"completions"),
            "generate missing completions subcommand"
        );
        assert!(
            sub_names.contains(&"man"),
            "generate missing man subcommand"
        );
    }
}
