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
    /// Customer retention cohorts.
    RetentionCohorts {
        #[arg(long)] mode: Option<String>,
        #[arg(long)] group_by: Option<String>,
        #[arg(long)] periods: Option<i64>,
        #[arg(long)] include_test: bool,
    },
    /// Uninstall reason breakdown.
    UninstallReasons {
        #[arg(long)] days: Option<i64>,
    },
    /// Active free trials.
    ActiveTrials {
        #[arg(long)] include_test: bool,
    },
    /// Churned customers.
    ChurnedCustomers {
        #[arg(long)] months: Option<i64>,
        #[arg(long)] page: Option<i64>,
        #[arg(long)] per_page: Option<i64>,
    },
    /// Store install records.
    StoreInstalls {
        #[arg(long)] status: Option<String>,
        #[arg(long)] channel: Option<String>,
        #[arg(long)] installed_after: Option<String>,
        #[arg(long)] installed_before: Option<String>,
        #[arg(long)] include_test: bool,
        #[arg(long)] page: Option<i64>,
        #[arg(long)] per_page: Option<i64>,
    },
    /// Customer lifecycle events.
    LifecycleEvents {
        #[arg(long)] event_type: Option<String>,
        #[arg(long)] event_date_after: Option<String>,
        #[arg(long)] event_date_before: Option<String>,
        #[arg(long)] include_test: bool,
        #[arg(long)] page: Option<i64>,
        #[arg(long)] per_page: Option<i64>,
    },
    /// Customer lifetime value.
    Ltv { #[command(subcommand)] cmd: LtvCmd },
    /// Store traffic analytics (BigQuery-backed).
    Traffic { #[command(subcommand)] cmd: TrafficCmd },
}

#[derive(Subcommand)] pub enum AppsCmd { List }
#[derive(Subcommand)] pub enum RankingsCmd {
    Get { /// Filter by keyword (API v1 has no keyword filter yet; this errors until it does).
        #[arg(long)] keyword: Option<String> },
    /// Scraped rank rows for a single keyword.
    ByKeyword { keyword: String, #[arg(long)] limit: Option<i64> },
}
#[derive(Subcommand)] pub enum KeywordsCmd {
    List,
    Track { keyword: String },
    Untrack { keyword: String },
    /// Keyword cannibalization report.
    Cannibalization { #[arg(long)] days: Option<i64> },
    /// Paid keyword ad performance.
    AdPerformance {
        #[arg(long)] start_date: Option<String>,
        #[arg(long)] end_date: Option<String>,
        #[arg(long)] min_pageviews: Option<i64>,
        #[arg(long)] sort_by: Option<String>,
        #[arg(long)] limit: Option<i64>,
    },
    /// Installs attributed to each keyword.
    InstallsBySource {
        #[arg(long)] start_date: Option<String>,
        #[arg(long)] end_date: Option<String>,
        #[arg(long)] keyword: Option<String>,
        #[arg(long)] sort_by: Option<String>,
        #[arg(long)] limit: Option<i64>,
    },
    /// Daily organic/sponsored trend for one keyword.
    Trends { keyword: String, #[arg(long)] days: Option<i64> },
}
#[derive(Subcommand)] pub enum ReviewsCmd { List }
#[derive(Subcommand)] pub enum InstallsCmd { Get }
#[derive(Subcommand)] pub enum ListingCmd { Get }
#[derive(Subcommand)] pub enum LtvCmd {
    /// Per-customer lifetime value.
    Customers {
        #[arg(long)] sort_by: Option<String>,
        #[arg(long)] page: Option<i64>,
        #[arg(long)] per_page: Option<i64>,
    },
    /// Lifetime value by acquisition cohort.
    Cohorts {
        #[arg(long)] page: Option<i64>,
        #[arg(long)] per_page: Option<i64>,
    },
}
#[derive(Subcommand)] pub enum TrafficCmd {
    /// Traffic overview series.
    Overview {
        #[arg(long)] start_date: Option<String>,
        #[arg(long)] end_date: Option<String>,
        #[arg(long)] granularity: Option<String>,
    },
    /// Install attribution by model.
    Attribution {
        #[arg(long)] start_date: Option<String>,
        #[arg(long)] end_date: Option<String>,
        #[arg(long)] model: Option<String>,
    },
    /// Conversion funnel.
    Funnel {
        #[arg(long)] start_date: Option<String>,
        #[arg(long)] end_date: Option<String>,
        #[arg(long)] include_test: bool,
    },
    /// Traffic source trends.
    SourceTrends {
        #[arg(long)] start_date: Option<String>,
        #[arg(long)] end_date: Option<String>,
        #[arg(long)] granularity: Option<String>,
    },
}
