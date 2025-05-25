use clap::Parser;
use nekotatsu::command::{run_command, Args};
use nekotatsu_core::tracing;
use tracing_subscriber;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let sub = tracing_subscriber::fmt()
        .with_file(false)
        .with_level(true)
        .with_target(false)
        .without_time()
        .compact()
        .finish();

    tracing::subscriber::set_global_default(sub)?;

    match args.command {
        Some(command) => {
            run_command(command)?;
        }
        None => {
            println!("Simple CLI tool that converts Neko backups into Kotatsu backups");
            println!("Run with -h for usage");
        }
    }
    Ok(())
}
