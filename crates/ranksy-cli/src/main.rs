mod cli;
mod config;
mod output;
mod run;

use clap::Parser;

fn main() {
    // Load a `.env` from the current directory (walking up parents) into the
    // process environment BEFORE clap parses, so `RANKSY_API_KEY` / `RANKSY_BASE_URL`
    // in a project-local .env are picked up by the `env` attrs on those flags.
    // Does not override a var already exported in the real environment.
    let _ = dotenvy::dotenv();

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
