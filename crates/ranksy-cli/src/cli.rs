use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "ranksy", version, about = "Run automations against the Ranksy API. An alternative to the MCP server.")]
pub struct Cli {
    /// Emit raw JSON instead of a table.
    #[arg(long, global = true)]
    pub json: bool,
    /// API key (overrides RANKSY_API_KEY and config).
    #[arg(long, global = true, env = "RANKSY_API_KEY", hide_env_values = true)]
    pub api_key: Option<String>,
    /// API base URL.
    #[arg(long, global = true, env = "RANKSY_BASE_URL")]
    pub base_url: Option<String>,
    /// Default app id.
    #[arg(long, global = true)]
    pub app: Option<String>,
    /// Re-run the command every N seconds.
    #[arg(long, global = true, value_name = "SECS")]
    pub watch: Option<u64>,
    /// Suppress non-essential output.
    #[arg(short, long, global = true)]
    pub quiet: bool,
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Save an API key to the local config.
    Login { key: String },
    /// Show the apps accessible with this key (the API has no dedicated whoami endpoint).
    Whoami,
    /// List your apps.
    Apps { #[command(subcommand)] cmd: AppsCmd },
    /// Keyword rankings.
    Rankings { #[command(subcommand)] cmd: RankingsCmd },
    /// Tracked keywords.
    Keywords { #[command(subcommand)] cmd: KeywordsCmd },
    /// App reviews.
    Reviews { #[command(subcommand)] cmd: ReviewsCmd },
    /// Install metrics.
    Installs { #[command(subcommand)] cmd: InstallsCmd },
    /// Store listing.
    Listing { #[command(subcommand)] cmd: ListingCmd },
}

#[derive(Subcommand)] pub enum AppsCmd { List }
#[derive(Subcommand)] pub enum RankingsCmd { Get { #[arg(long)] keyword: Option<String> } }
#[derive(Subcommand)] pub enum KeywordsCmd {
    List,
    Track { keyword: String },
    Untrack { keyword: String },
}
#[derive(Subcommand)] pub enum ReviewsCmd { List }
#[derive(Subcommand)] pub enum InstallsCmd { Get }
#[derive(Subcommand)] pub enum ListingCmd { Get }
