use clap::Parser;
use bridge::cli::{self, Args, CommandOutcome};
use bridge::daemon;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    if let Some(command) = args.command {
        match cli::execute_command(command, args.config.as_deref()).await? {
            CommandOutcome::ContinueWithConfig(cfg) => daemon::run(cfg.as_deref()).await,
            CommandOutcome::Exit => Ok(()),
        }
    } else {
        daemon::run(args.config.as_deref()).await
    }
}
