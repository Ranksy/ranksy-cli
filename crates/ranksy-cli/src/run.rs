use crate::cli::{Cli, Commands};
use crate::{config, output};
use anyhow::{anyhow, Result};
use ranksy_api::{ClientConfig, RanksyClient};

pub async fn run(cli: Cli) -> Result<()> {
    // login writes config and returns before any auth resolution.
    if let Commands::Login { key } = &cli.command {
        config::save_api_key(key)?;
        if !cli.quiet {
            println!("Saved API key to {}", config::config_path().display());
        }
        return Ok(());
    }

    // self-update needs no API key or app; handle it before auth resolution.
    if let Commands::Update { check } = &cli.command {
        return run_update(*check, cli.quiet).await;
    }

    let file = config::load();
    let resolved = config::resolve(
        cli.api_key.clone(),
        None, // env already folded in by clap's `env` attr on api_key
        cli.base_url.clone(),
        None,
        cli.app.clone(),
        &file,
    )
    .map_err(|e| anyhow!(e.to_string()))?;

    let client = RanksyClient::new(ClientConfig {
        api_key: resolved.api_key.clone(),
        base_url: resolved.base_url.clone(),
    })?;

    let format = if cli.json { output::Format::Json } else { output::Format::Table };
    let app = resolved.app.clone();

    loop {
        let (value, columns) = dispatch(&client, &cli.command, app.as_deref()).await?;
        println!("{}", output::render(&value, format, &columns));
        match cli.watch {
            Some(secs) if secs > 0 => tokio::time::sleep(std::time::Duration::from_secs(secs)).await,
            _ => break,
        }
    }
    Ok(())
}

async fn dispatch(
    client: &RanksyClient,
    command: &Commands,
    app: Option<&str>,
) -> Result<(serde_json::Value, Vec<output::Column>)> {
    use crate::cli::*;
    let need_app = || app.ok_or_else(|| anyhow!("no app selected. Pass --app <id> or set it in config."));
    let (value, columns) = match command {
        Commands::Login { .. } | Commands::Update { .. } => unreachable!("handled earlier"),
        Commands::Whoami => (
            client.whoami().await?,
            vec![col("Ulid", "ulid"), col("Slug", "slug"), col("Name", "name")],
        ),
        Commands::Apps { cmd: AppsCmd::List } => (
            client.list_apps().await?,
            vec![col("Ulid", "ulid"), col("Slug", "slug"), col("Name", "name")],
        ),
        Commands::Rankings { cmd } => match cmd {
            RankingsCmd::Get { keyword } => (
                client.get_rankings(need_app()?, keyword.as_deref()).await?,
                vec![col("Rank", "rank"), col("Change", "change"), col("Date", "date")],
            ),
            RankingsCmd::ByKeyword { keyword, limit } => {
                let mut q: Vec<(&str, String)> = Vec::new();
                push_int(&mut q, "limit", limit);
                let qq = qref(&q);
                (
                    client.get_app(need_app()?, &format!("rankings/by-keyword/{keyword}"), &qq).await?,
                    vec![col("Rank", "rank"), col("Change", "change"), col("Date", "date")],
                )
            }
        },
        Commands::Keywords { cmd } => match cmd {
            KeywordsCmd::List => (
                client.list_keywords(need_app()?).await?,
                vec![col("Keyword", "keyword"), col("Organic rank", "organic_rank"), col("Sponsored rank", "sponsored_rank")],
            ),
            KeywordsCmd::Track { keyword } => (client.track_keyword(need_app()?, keyword).await?, vec![]),
            KeywordsCmd::Untrack { keyword } => (client.untrack_keyword(need_app()?, keyword).await?, vec![]),
            KeywordsCmd::Cannibalization { days } => {
                let mut q: Vec<(&str, String)> = Vec::new();
                push_int(&mut q, "days", days);
                let qq = qref(&q);
                (client.get_app(need_app()?, "keywords/cannibalization", &qq).await?, vec![])
            }
            KeywordsCmd::AdPerformance { start_date, end_date, min_pageviews, sort_by, limit } => {
                let mut q: Vec<(&str, String)> = Vec::new();
                push_opt(&mut q, "start_date", start_date);
                push_opt(&mut q, "end_date", end_date);
                push_int(&mut q, "min_pageviews", min_pageviews);
                push_opt(&mut q, "sort_by", sort_by);
                push_int(&mut q, "limit", limit);
                let qq = qref(&q);
                (client.get_app(need_app()?, "keywords/ad-performance", &qq).await?, vec![])
            }
            KeywordsCmd::InstallsBySource { start_date, end_date, keyword, sort_by, limit } => {
                let mut q: Vec<(&str, String)> = Vec::new();
                push_opt(&mut q, "start_date", start_date);
                push_opt(&mut q, "end_date", end_date);
                push_opt(&mut q, "keyword", keyword);
                push_opt(&mut q, "sort_by", sort_by);
                push_int(&mut q, "limit", limit);
                let qq = qref(&q);
                (client.get_app(need_app()?, "keywords/installs-by-source", &qq).await?, vec![])
            }
            KeywordsCmd::Trends { keyword, days } => {
                let mut q: Vec<(&str, String)> = Vec::new();
                push_int(&mut q, "days", days);
                let qq = qref(&q);
                (client.get_app(need_app()?, &format!("keywords/{keyword}/trends"), &qq).await?, vec![])
            }
        },
        Commands::Reviews { cmd: ReviewsCmd::List } => (
            client.list_reviews(need_app()?).await?,
            vec![col("Rating", "rating"), col("Author", "author"), col("Body", "body")],
        ),
        Commands::Installs { cmd: InstallsCmd::Get } => (
            client.get_installs(need_app()?).await?,
            vec![col("Source", "source"), col("Installs", "installs"), col("Share", "share")],
        ),
        Commands::Listing { cmd: ListingCmd::Get } => (client.get_listing(need_app()?).await?, vec![]),
        Commands::RetentionCohorts { mode, group_by, periods, include_test } => {
            let mut q: Vec<(&str, String)> = Vec::new();
            push_opt(&mut q, "mode", mode);
            push_opt(&mut q, "group_by", group_by);
            push_int(&mut q, "periods", periods);
            push_bool(&mut q, "include_test", *include_test);
            let qq = qref(&q);
            (client.get_app(need_app()?, "retention/cohorts", &qq).await?, vec![])
        }
        Commands::UninstallReasons { days } => {
            let mut q: Vec<(&str, String)> = Vec::new();
            push_int(&mut q, "days", days);
            let qq = qref(&q);
            (client.get_app(need_app()?, "uninstall-reasons", &qq).await?, vec![])
        }
        Commands::ActiveTrials { include_test } => {
            let mut q: Vec<(&str, String)> = Vec::new();
            push_bool(&mut q, "include_test", *include_test);
            let qq = qref(&q);
            (client.get_app(need_app()?, "active-trials", &qq).await?, vec![])
        }
        Commands::ChurnedCustomers { months, page, per_page } => {
            let mut q: Vec<(&str, String)> = Vec::new();
            push_int(&mut q, "months", months);
            push_int(&mut q, "page", page);
            push_int(&mut q, "per_page", per_page);
            let qq = qref(&q);
            (client.get_app(need_app()?, "churned-customers", &qq).await?, vec![])
        }
        Commands::StoreInstalls { status, channel, installed_after, installed_before, include_test, page, per_page } => {
            let mut q: Vec<(&str, String)> = Vec::new();
            push_opt(&mut q, "status", status);
            push_opt(&mut q, "channel", channel);
            push_opt(&mut q, "installed_after", installed_after);
            push_opt(&mut q, "installed_before", installed_before);
            push_bool(&mut q, "include_test", *include_test);
            push_int(&mut q, "page", page);
            push_int(&mut q, "per_page", per_page);
            let qq = qref(&q);
            (client.get_app(need_app()?, "store-installs", &qq).await?, vec![])
        }
        Commands::LifecycleEvents { event_type, event_date_after, event_date_before, include_test, page, per_page } => {
            let mut q: Vec<(&str, String)> = Vec::new();
            push_opt(&mut q, "event_type", event_type);
            push_opt(&mut q, "event_date_after", event_date_after);
            push_opt(&mut q, "event_date_before", event_date_before);
            push_bool(&mut q, "include_test", *include_test);
            push_int(&mut q, "page", page);
            push_int(&mut q, "per_page", per_page);
            let qq = qref(&q);
            (client.get_app(need_app()?, "lifecycle-events", &qq).await?, vec![])
        }
        Commands::Ltv { cmd } => match cmd {
            LtvCmd::Customers { sort_by, page, per_page } => {
                let mut q: Vec<(&str, String)> = Vec::new();
                push_opt(&mut q, "sort_by", sort_by);
                push_int(&mut q, "page", page);
                push_int(&mut q, "per_page", per_page);
                let qq = qref(&q);
                (client.get_app(need_app()?, "ltv/customers", &qq).await?, vec![])
            }
            LtvCmd::Cohorts { page, per_page } => {
                let mut q: Vec<(&str, String)> = Vec::new();
                push_int(&mut q, "page", page);
                push_int(&mut q, "per_page", per_page);
                let qq = qref(&q);
                (client.get_app(need_app()?, "ltv/cohorts", &qq).await?, vec![])
            }
        },
        Commands::Traffic { cmd } => match cmd {
            TrafficCmd::Overview { start_date, end_date, granularity } => {
                let mut q: Vec<(&str, String)> = Vec::new();
                push_opt(&mut q, "start_date", start_date);
                push_opt(&mut q, "end_date", end_date);
                push_opt(&mut q, "granularity", granularity);
                let qq = qref(&q);
                (client.get_app(need_app()?, "traffic/overview", &qq).await?, vec![])
            }
            TrafficCmd::Attribution { start_date, end_date, model } => {
                let mut q: Vec<(&str, String)> = Vec::new();
                push_opt(&mut q, "start_date", start_date);
                push_opt(&mut q, "end_date", end_date);
                push_opt(&mut q, "model", model);
                let qq = qref(&q);
                (client.get_app(need_app()?, "traffic/attribution", &qq).await?, vec![])
            }
            TrafficCmd::Funnel { start_date, end_date, include_test } => {
                let mut q: Vec<(&str, String)> = Vec::new();
                push_opt(&mut q, "start_date", start_date);
                push_opt(&mut q, "end_date", end_date);
                push_bool(&mut q, "include_test", *include_test);
                let qq = qref(&q);
                (client.get_app(need_app()?, "traffic/funnel", &qq).await?, vec![])
            }
            TrafficCmd::SourceTrends { start_date, end_date, granularity } => {
                let mut q: Vec<(&str, String)> = Vec::new();
                push_opt(&mut q, "start_date", start_date);
                push_opt(&mut q, "end_date", end_date);
                push_opt(&mut q, "granularity", granularity);
                let qq = qref(&q);
                (client.get_app(need_app()?, "traffic/source-trends", &qq).await?, vec![])
            }
        },
    };
    Ok((value, columns))
}

fn col(header: &'static str, key: &'static str) -> output::Column {
    output::Column { header, key }
}

fn push_opt<'a>(q: &mut Vec<(&'a str, String)>, key: &'a str, value: &Option<String>) {
    if let Some(v) = value {
        q.push((key, v.clone()));
    }
}

fn push_int<'a>(q: &mut Vec<(&'a str, String)>, key: &'a str, value: &Option<i64>) {
    if let Some(v) = value {
        q.push((key, v.to_string()));
    }
}

fn push_bool<'a>(q: &mut Vec<(&'a str, String)>, key: &'a str, value: bool) {
    if value {
        q.push((key, "true".to_string()));
    }
}

fn qref<'a>(q: &'a [(&'a str, String)]) -> Vec<(&'a str, &'a str)> {
    q.iter().map(|(k, v)| (*k, v.as_str())).collect()
}

const INSTALL_HINT: &str = "curl --proto '=https' --tlsv1.2 -LsSf https://github.com/ranksy/ranksy-cli/releases/latest/download/ranksy-cli-installer.sh | sh";

/// `ranksy update [--check]`. Uses the install receipt written by the cargo-dist
/// installer; builds installed another way (e.g. `cargo install`) have no
/// receipt, so we point the user at the installer instead of failing obscurely.
async fn run_update(check_only: bool, quiet: bool) -> Result<()> {
    use axoupdater::AxoUpdater;

    let current = env!("CARGO_PKG_VERSION");
    let mut updater = AxoUpdater::new_for("ranksy-cli");
    if updater.load_receipt().is_err() {
        return Err(anyhow!(
            "can't self-update: no install receipt found (this build wasn't installed via the release installer).\nInstall or update with:\n  {INSTALL_HINT}"
        ));
    }

    if check_only {
        if updater.is_update_needed().await? {
            let latest = updater
                .query_new_version()
                .await?
                .map(|v| v.to_string())
                .unwrap_or_else(|| "newer".to_string());
            println!("Update available: v{latest} (current v{current}). Run `ranksy update`.");
        } else {
            println!("Up to date (v{current}).");
        }
        return Ok(());
    }

    match updater.run().await? {
        Some(result) => {
            if !quiet {
                println!("Updated v{current} -> v{}.", result.new_version);
            }
        }
        None => {
            if !quiet {
                println!("Already up to date (v{current}).");
            }
        }
    }
    Ok(())
}
