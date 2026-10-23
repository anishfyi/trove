use std::process::ExitCode;

use clap::Parser;
use trove_ship::cli::{run, ShipCli};

fn main() -> ExitCode {
    let cli = ShipCli::parse();
    ExitCode::from(run(&cli, "trove-ship"))
}
