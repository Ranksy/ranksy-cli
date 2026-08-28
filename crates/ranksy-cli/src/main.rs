use clap::Parser;

mod config;

/// Ranksy CLI — run automations against the Ranksy API. An alternative to the MCP server.
#[derive(Parser)]
#[command(name = "ranksy", version, about)]
struct Cli {}

fn main() {
    let _ = Cli::parse();
}
