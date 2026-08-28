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
        Commands::Login { .. } => unreachable!("handled earlier"),
        Commands::Whoami => (
            client.whoami().await?,
            vec![col("Ulid", "ulid"), col("Slug", "slug"), col("Name", "name")],
        ),
        Commands::Apps { cmd: AppsCmd::List } => (
            client.list_apps().await?,
            vec![col("Ulid", "ulid"), col("Slug", "slug"), col("Name", "name")],
        ),
        Commands::Rankings { cmd: RankingsCmd::Get { keyword } } => (
            client.get_rankings(need_app()?, keyword.as_deref()).await?,
            vec![col("Rank", "rank"), col("Change", "change"), col("Date", "date")],
        ),
        Commands::Keywords { cmd } => match cmd {
            KeywordsCmd::List => (
                client.list_keywords(need_app()?).await?,
                vec![col("Keyword", "keyword"), col("Organic rank", "organic_rank"), col("Sponsored rank", "sponsored_rank")],
            ),
            KeywordsCmd::Track { keyword } => (client.track_keyword(need_app()?, keyword).await?, vec![]),
            KeywordsCmd::Untrack { keyword } => (client.untrack_keyword(need_app()?, keyword).await?, vec![]),
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
    };
    Ok((value, columns))
}

fn col(header: &'static str, key: &'static str) -> output::Column {
    output::Column { header, key }
}
