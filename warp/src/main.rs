
pub mod terminal;
pub mod http;

use warp_common::{
	tokio,
	anyhow
};
use clap::Parser;

#[derive(Debug, Parser)]
#[clap(version, about, long_about = None)]
struct CommandArgs {
	#[clap(short, long)]
    verbose: bool,
    //TODO: Make into a separate subcommand
    #[clap(long)]
    http: bool,
    #[clap(long)]
    ui: bool,
    #[clap(long)]
    cli: bool,
    #[clap(short, long)]
    config: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
	let cli = CommandArgs::parse();

	//TODO: Implement configuration

	match (cli.ui, cli.cli, cli.http) {
	    (true, false, false) => todo!(),
	    (false, true, false) => todo!(),
	    (false, false, true) => http::http_main().await?,
	    (false, false, false) => {},
	    _ => println!("You can only select one option")
	};
	Ok(())
}

