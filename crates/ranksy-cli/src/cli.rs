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
    Login {
        /// API key from Settings > API Keys (`rk_live_...` or `rk_test_...`).
        key: String,
    },
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
        /// Cohort basis: `install` or `subscription` [default: install].
        #[arg(long)] mode: Option<String>,
        /// Cohort size: `day`, `week` or `month` [default: month].
        #[arg(long)] group_by: Option<String>,
        /// Periods per cohort; capped at 60 days, 52 weeks or 36 months [default: 12].
        #[arg(long)] periods: Option<i64>,
        /// Include test and development stores.
        #[arg(long)] include_test: bool,
    },
    /// Uninstall reason breakdown.
    UninstallReasons {
        /// Days to look back, 1 to 90 [default: 30].
        #[arg(long)] days: Option<i64>,
    },
    /// Active free trials.
    ActiveTrials {
        /// Include test and development stores.
        #[arg(long)] include_test: bool,
    },
    /// Churned customers.
    ChurnedCustomers {
        /// Months to look back, 1 to 12 [default: 6].
        #[arg(long)] months: Option<i64>,
        /// Page number [default: 1].
        #[arg(long)] page: Option<i64>,
        /// Rows per page, 1 to 100 [default: 50].
        #[arg(long)] per_page: Option<i64>,
    },
    /// Store install records.
    StoreInstalls {
        /// `INSTALLED`, `UNINSTALLED`, `CLOSED`, `FROZEN` or `all` [default: INSTALLED].
        #[arg(long)] status: Option<String>,
        /// Only stores from this first-touch channel.
        #[arg(long)] channel: Option<String>,
        /// Only stores installed on or after this date, YYYY-MM-DD.
        #[arg(long)] installed_after: Option<String>,
        /// Only stores installed on or before this date, YYYY-MM-DD.
        #[arg(long)] installed_before: Option<String>,
        /// Include test and development stores.
        #[arg(long)] include_test: bool,
        /// Page number [default: 1].
        #[arg(long)] page: Option<i64>,
        /// Rows per page, 1 to 100 [default: 50].
        #[arg(long)] per_page: Option<i64>,
    },
    /// Customer lifecycle events.
    LifecycleEvents {
        /// Only this event type, for example `INSTALLED`, `TRIAL_STARTED`, `SUBSCRIBED` or `CHURNED`.
        #[arg(long)] event_type: Option<String>,
        /// Only events on or after this date, YYYY-MM-DD [default: 30 days ago].
        #[arg(long)] event_date_after: Option<String>,
        /// Only events on or before this date, YYYY-MM-DD [default: today].
        #[arg(long)] event_date_before: Option<String>,
        /// Include test and development stores.
        #[arg(long)] include_test: bool,
        /// Page number [default: 1].
        #[arg(long)] page: Option<i64>,
        /// Rows per page, 1 to 100 [default: 50].
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
    /// Generate documentation from these command definitions.
    #[command(hide = true)]
    Docs { #[command(subcommand)] cmd: DocsCmd },
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
    /// Chrome profile directory that keeps the Partners login (default: `<data dir>/partners-chrome`).
    #[arg(long, value_name = "DIR")]
    pub profile: Option<PathBuf>,
}

#[derive(Subcommand)]
pub enum AdsCmd {
    /// Log in to Shopify Partners and map the Shopify apps of one organization to Ranksy apps
    /// (interactive). Run it once per Partners organization; each run adds to the connected ones.
    Connect {
        #[command(flatten)]
        chrome: ChromeArgs,
        /// Partners organization id to add (default: detected after login; asked when you have several).
        #[arg(long)]
        org: Option<String>,
    },
    /// Stop syncing a Partners organization and forget its app mappings.
    Disconnect {
        /// Partners organization id to remove.
        #[arg(long)]
        org: String,
    },
    /// Pull ads data from Shopify Partners and push it to Ranksy (non-interactive; for cron).
    /// Syncs every connected organization, one after the other, unless --org picks one.
    Sync {
        #[command(flatten)]
        chrome: ChromeArgs,
        /// Only sync this connected Partners organization (default: all of them).
        #[arg(long)]
        org: Option<String>,
        /// Only sync this mapped app, by Ranksy app id, Shopify app id (API key) or Shopify app name (case-insensitive). Repeat it for several apps; combines with --org. The organization's daily campaign list is still pulled; other apps' breakdowns, search terms and keywords are skipped, and nothing is pushed for them (default: all mapped apps).
        #[arg(long, value_name = "APP")]
        only_app: Vec<String>,
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
        /// Directory for the raw day exports and payload files, one subfolder per organization (default: `<cache dir>/ads-export`).
        #[arg(long, value_name = "DIR")]
        out: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
pub enum DocsCmd {
    /// Print the command reference as MDX for api.ranksyapp.com.
    Markdown,
}

#[derive(Subcommand)] pub enum AppsCmd {
    /// List the apps of the key's team, with their ids.
    List,
}
#[derive(Subcommand)] pub enum RankingsCmd {
    /// Current keyword rankings of the app.
    Get { /// Narrow the snapshot to one keyword (text or slug); same data as `rankings by-keyword`.
        #[arg(long)] keyword: Option<String> },
    /// Scraped rank rows for a single keyword.
    ByKeyword {
        /// Keyword text or slug.
        keyword: String,
        /// Rows to return, 1 to 50 [default: 20].
        #[arg(long)] limit: Option<i64>,
    },
}
#[derive(Subcommand)] pub enum KeywordsCmd {
    /// Keywords the app ranks for, with organic and sponsored rank.
    List,
    /// Start tracking a keyword (needs `keywords:write`).
    Track {
        /// Keyword text.
        keyword: String,
    },
    /// Stop tracking a keyword; accepts the keyword text or its slug (needs `keywords:write`).
    Untrack {
        /// Keyword text or slug.
        keyword: String,
    },
    /// Keyword cannibalization report.
    Cannibalization {
        /// Days to look back, 1 to 365 [default: 30].
        #[arg(long)] days: Option<i64>,
    },
    /// Paid keyword ad performance.
    AdPerformance {
        /// First day, YYYY-MM-DD [default: 30 days ago].
        #[arg(long)] start_date: Option<String>,
        /// Last day, YYYY-MM-DD [default: today].
        #[arg(long)] end_date: Option<String>,
        /// Skip keywords with fewer page views [default: 0].
        #[arg(long)] min_pageviews: Option<i64>,
        /// `installs`, `pageviews` or `conversion_rate` [default: installs].
        #[arg(long)] sort_by: Option<String>,
        /// Keywords to return, 1 to 50 [default: 30].
        #[arg(long)] limit: Option<i64>,
    },
    /// Installs attributed to each keyword.
    InstallsBySource {
        /// First day, YYYY-MM-DD [default: 30 days ago].
        #[arg(long)] start_date: Option<String>,
        /// Last day, YYYY-MM-DD [default: today].
        #[arg(long)] end_date: Option<String>,
        /// Only this keyword.
        #[arg(long)] keyword: Option<String>,
        /// `total_installs`, `organic_installs` or `ad_installs` [default: total_installs].
        #[arg(long)] sort_by: Option<String>,
        /// Keywords to return, 1 to 50 [default: 30].
        #[arg(long)] limit: Option<i64>,
    },
    /// Daily organic/sponsored trend for one keyword.
    Trends {
        /// Keyword text, matched exactly.
        keyword: String,
        /// Days to look back, 1 to 90 [default: 14].
        #[arg(long)] days: Option<i64>,
    },
}
#[derive(Subcommand)] pub enum ReviewsCmd {
    /// The app's App Store reviews.
    List,
}
#[derive(Subcommand)] pub enum InstallsCmd {
    /// Installs attributed by traffic source.
    Get,
}
#[derive(Subcommand)] pub enum ListingCmd {
    /// Fetch the app's store listing (needs `apps:read`).
    Get,
}
#[derive(Subcommand)] pub enum LtvCmd {
    /// Per-customer lifetime value.
    Customers {
        /// `ltv`, `current_mrr` or `installed_at` [default: ltv].
        #[arg(long)] sort_by: Option<String>,
        /// Page number [default: 1].
        #[arg(long)] page: Option<i64>,
        /// Rows per page, 1 to 100 [default: 50].
        #[arg(long)] per_page: Option<i64>,
    },
    /// Lifetime value by acquisition cohort.
    Cohorts {
        /// Page number [default: 1].
        #[arg(long)] page: Option<i64>,
        /// Rows per page, 1 to 100 [default: 100].
        #[arg(long)] per_page: Option<i64>,
    },
}
#[derive(Subcommand)] pub enum TrafficCmd {
    /// Traffic overview series.
    Overview {
        /// First day, YYYY-MM-DD [default: 30 days ago].
        #[arg(long)] start_date: Option<String>,
        /// Last day, YYYY-MM-DD [default: today].
        #[arg(long)] end_date: Option<String>,
        /// `daily` or `weekly` [default: weekly for ranges over 14 days, else daily].
        #[arg(long)] granularity: Option<String>,
    },
    /// Install attribution by model.
    Attribution {
        /// First day, YYYY-MM-DD [default: 30 days ago].
        #[arg(long)] start_date: Option<String>,
        /// Last day, YYYY-MM-DD [default: today].
        #[arg(long)] end_date: Option<String>,
        /// `first_touch`, `last_touch`, `linear`, `time_decay` or `position_based` [default: last_touch].
        #[arg(long)] model: Option<String>,
    },
    /// Conversion funnel.
    Funnel {
        /// First day, YYYY-MM-DD [default: 30 days ago].
        #[arg(long)] start_date: Option<String>,
        /// Last day, YYYY-MM-DD [default: today].
        #[arg(long)] end_date: Option<String>,
        /// Include test and development stores.
        #[arg(long)] include_test: bool,
    },
    /// Traffic source trends.
    SourceTrends {
        /// First day, YYYY-MM-DD [default: 30 days ago].
        #[arg(long)] start_date: Option<String>,
        /// Last day, YYYY-MM-DD [default: today].
        #[arg(long)] end_date: Option<String>,
        /// `weekly` or `biweekly` [default: weekly].
        #[arg(long)] granularity: Option<String>,
    },
}
