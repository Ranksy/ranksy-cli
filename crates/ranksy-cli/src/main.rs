mod cli;
mod config;
mod output;
mod run;

use clap::Parser;

fn main() {
    let parsed = cli::Cli::parse(); // clap exits 2 on usage error
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    match rt.block_on(run::run(parsed)) {
        Ok(()) => {}
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}
