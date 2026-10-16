use chrono::NaiveDate;
use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

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
    /// Shopify App Store ads: pull from Shopify Partners into Ranksy.
    Ads { #[command(subcommand)] cmd: AdsCmd },
    /// Update ranksy to the latest release (or `--check` to only look).
    Update {
        /// Report whether a newer version exists without installing it.
        #[arg(long)]
        check: bool,
    },
}

/// How to reach the Chrome window that holds the Shopify Partners login.
#[derive(Args, Clone, Debug)]
pub struct ChromeArgs {
    /// Chrome binary (default: google-chrome, google-chrome-stable, chromium, chromium-browser on PATH).
    #[arg(long, env = "CHROME_BIN", value_name = "PATH")]
    pub chrome: Option<PathBuf>,
    /// Chrome remote-debugging port (default 9333; an existing Chrome on it is reused).
    #[arg(long)]
    pub port: Option<u16>,
    /// Chrome profile directory that keeps the Partners login (default: <data dir>/partners-chrome).
    #[arg(long, value_name = "DIR")]
    pub profile: Option<PathBuf>,
}

#[derive(Subcommand)]
pub enum AdsCmd {
    /// Log in to Shopify Partners once and map your Shopify apps to Ranksy apps (interactive).
    Connect {
        #[command(flatten)]
        chrome: ChromeArgs,
        /// Partners organization id (default: detected after login; asked when you have several).
        #[arg(long)]
        org: Option<String>,
    },
    /// Pull ads data from Shopify Partners and push it to Ranksy (non-interactive; for cron).
    Sync {
        #[command(flatten)]
        chrome: ChromeArgs,
        /// First day to pull, YYYY-MM-DD (default: --days before --to).
        #[arg(long, value_name = "YYYY-MM-DD", value_parser = crate::ads::parse_date)]
        from: Option<NaiveDate>,
        /// Last day to pull, YYYY-MM-DD (default: yesterday, UTC).
        #[arg(long, value_name = "YYYY-MM-DD", value_parser = crate::ads::parse_date)]
        to: Option<NaiveDate>,
        /// Only days after this date (exclusive), YYYY-MM-DD; alone, pulls up to yesterday.
        #[arg(long, value_name = "YYYY-MM-DD", value_parser = crate::ads::parse_date, conflicts_with = "from")]
        after: Option<NaiveDate>,
        /// Only days before this date (exclusive), YYYY-MM-DD.
        #[arg(long, value_name = "YYYY-MM-DD", value_parser = crate::ads::parse_date, conflicts_with = "to")]
        before: Option<NaiveDate>,
        /// Number of complete days ending at --to, when --from is not given [default: 3].
        #[arg(long, value_name = "N", value_parser = clap::value_parser!(u32).range(1..))]
        days: Option<u32>,
        /// Days pulled per push to Ranksy (newest first).
        #[arg(long, value_name = "N", default_value_t = 7, value_parser = clap::value_parser!(u32).range(1..))]
        chunk_days: u32,
        /// Days pulled from Partners in parallel.
        #[arg(long, value_name = "N", default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
        concurrency: u32,
        /// Pull and build the payloads, write them to --out, but don't POST to Ranksy.
        #[arg(long)]
        dry_run: bool,
        /// Reuse days already pulled into --out instead of pulling them again.
        #[arg(long)]
        resume: bool,
        /// Directory for the raw day exports and payload files (default: <cache dir>/ads-export).
        #[arg(long, value_name = "DIR")]
        out: Option<PathBuf>,
    },
}

#[derive(Subcommand)] pub enum AppsCmd { List }
#[derive(Subcommand)] pub enum RankingsCmd {
    Get { /// Narrow the snapshot to one keyword (text or slug); same data as `rankings by-keyword`.
        #[arg(long)] keyword: Option<String> },
    /// Scraped rank rows for a single keyword.
    ByKeyword { keyword: String, #[arg(long)] limit: Option<i64> },
}
#[derive(Subcommand)] pub enum KeywordsCmd {
    List,
    /// Start tracking a keyword (needs `keywords:write`).
    Track { keyword: String },
    /// Stop tracking a keyword; accepts the keyword text or its slug (needs `keywords:write`).
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
#[derive(Subcommand)] pub enum ListingCmd {
    /// Fetch the app's store listing (needs `apps:read`).
    Get,
}
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
