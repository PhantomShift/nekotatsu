use clap::Parser;
use nekotatsu::Args;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    match args.command {
        Some(command) => {
            nekotatsu::run_command(command)?;
        }
        None => {
            println!("Simple CLI tool that converts Neko backups into Kotatsu backups");
            println!("Run with -h for usage");
        }
    }
    
    Ok(())
}