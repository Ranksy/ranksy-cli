//! `ranksy docs markdown`: the command reference published at api.ranksyapp.com/cli/commands.
//!
//! It is built from the clap definitions, so flags, defaults and help text can't drift from the
//! binary. What clap doesn't know, the API endpoint and scope behind each command, lives in
//! `REFERENCE`; a test fails when a command has no entry there.

use crate::cli::Cli;
use clap::{Arg, ArgAction, Command, CommandFactory};

const OPENAPI_URL: &str = "https://ranksyapp.com/openapi.json";

/// What a command calls in the Ranksy API, and where the API docs describe it.
struct Ref {
    /// The command path without `ranksy`, e.g. `rankings get`.
    command: &'static str,
    /// HTTP method and path, or `None` when the command makes no API call.
    endpoint: Option<(&'static str, &'static str)>,
    scope: Option<&'static str>,
    /// Title and link of the endpoint's section on api.ranksyapp.com.
    docs: Option<(&'static str, &'static str)>,
    note: Option<&'static str>,
}

const fn api(
    command: &'static str,
    method: &'static str,
    path: &'static str,
    scope: &'static str,
    docs: Option<(&'static str, &'static str)>,
) -> Ref {
    Ref { command, endpoint: Some((method, path)), scope: Some(scope), docs, note: None }
}

const fn local(command: &'static str, note: &'static str) -> Ref {
    Ref { command, endpoint: None, scope: None, docs: None, note: Some(note) }
}

const fn with_note(r: Ref, note: &'static str) -> Ref {
    Ref { note: Some(note), ..r }
}

const LIST_APPS: Option<(&str, &str)> = Some(("List apps", "/apps#list-apps"));
const ADS_GUIDE: Option<(&str, &str)> = Some(("Sync App Store Ads", "/cli/ads"));

const REFERENCE: &[Ref] = &[
    local("login", "Writes the key to the config file. Makes no API call."),
    api("whoami", "GET", "/api/v1/apps", "apps:read", LIST_APPS),
    api("apps list", "GET", "/api/v1/apps", "apps:read", LIST_APPS),
    with_note(
        api("rankings get", "GET", "/api/v1/apps/:app/rankings", "rankings:read", Some(("Current rankings", "/rankings#current-rankings"))),
        "With `--keyword`, it calls `GET /api/v1/apps/:app/rankings/by-keyword/:keyword` instead.",
    ),
    api("rankings by-keyword", "GET", "/api/v1/apps/:app/rankings/by-keyword/:keyword", "rankings:read", None),
    api("keywords list", "GET", "/api/v1/apps/:app/keywords", "traffic:read", Some(("Keyword performance", "/traffic#keyword-performance"))),
    api("keywords track", "POST", "/api/v1/apps/:app/keywords", "keywords:write", None),
    with_note(
        api("keywords untrack", "DELETE", "/api/v1/apps/:app/keywords/:keyword", "keywords:write", None),
        "The keyword is turned into its slug before the call, so `\"Email Marketing\"` and `email-marketing` both work.",
    ),
    api("keywords cannibalization", "GET", "/api/v1/apps/:app/keywords/cannibalization", "traffic:read", None),
    api("keywords ad-performance", "GET", "/api/v1/apps/:app/keywords/ad-performance", "traffic:read", None),
    api("keywords installs-by-source", "GET", "/api/v1/apps/:app/keywords/installs-by-source", "traffic:read", None),
    api("keywords trends", "GET", "/api/v1/apps/:app/keywords/:keyword/trends", "traffic:read", None),
    api("reviews list", "GET", "/api/v1/apps/:app/reviews", "reviews:read", Some(("App Store reviews", "/reviews#app-store-reviews"))),
    api("installs get", "GET", "/api/v1/apps/:app/installs", "traffic:read", Some(("Attributed installs", "/traffic#attributed-installs"))),
    api("listing get", "GET", "/api/v1/apps/:app/listing", "apps:read", None),
    api("retention-cohorts", "GET", "/api/v1/apps/:app/retention/cohorts", "metrics:read", None),
    api("uninstall-reasons", "GET", "/api/v1/apps/:app/uninstall-reasons", "metrics:read", None),
    api("active-trials", "GET", "/api/v1/apps/:app/active-trials", "metrics:read", None),
    api("churned-customers", "GET", "/api/v1/apps/:app/churned-customers", "metrics:read", Some(("Churned customers", "/churned-customers#churned-customers"))),
    api("store-installs", "GET", "/api/v1/apps/:app/store-installs", "metrics:read", Some(("Store installs", "/store-installs#store-installs"))),
    api("lifecycle-events", "GET", "/api/v1/apps/:app/lifecycle-events", "metrics:read", Some(("Lifecycle events", "/lifecycle-events#lifecycle-events"))),
    api("ltv customers", "GET", "/api/v1/apps/:app/ltv/customers", "metrics:read", Some(("Customer LTV", "/customer-ltv#customer-ltv"))),
    api("ltv cohorts", "GET", "/api/v1/apps/:app/ltv/cohorts", "metrics:read", Some(("LTV cohorts", "/ltv-cohorts#ltv-cohorts"))),
    api("traffic overview", "GET", "/api/v1/apps/:app/traffic/overview", "traffic:read", None),
    api("traffic attribution", "GET", "/api/v1/apps/:app/traffic/attribution", "traffic:read", None),
    api("traffic funnel", "GET", "/api/v1/apps/:app/traffic/funnel", "traffic:read", None),
    api("traffic source-trends", "GET", "/api/v1/apps/:app/traffic/source-trends", "traffic:read", None),
    with_note(
        api("ads connect", "GET", "/api/v1/apps", "apps:read", ADS_GUIDE),
        "It lists your Ranksy apps so you can map each Shopify app to one. Give the same key `ads:write` too, for `ads sync`.",
    ),
    local("ads disconnect", "Edits the config file. Makes no API call."),
    with_note(
        api("ads sync", "POST", "/api/v1/apps/:app/shopify-ads/sync", "ads:write", ADS_GUIDE),
        "One call per mapped app for each chunk of days. With `--dry-run` it makes no API call and needs no key.",
    ),
    local("update", "Downloads the latest release from GitHub. Makes no Ranksy API call."),
];

/// The command reference as MDX: global options, then one section per command.
pub fn markdown() -> String {
    let mut root = Cli::command();
    root.build();

    let mut out = String::new();
    out.push_str("## Global options\n\n");
    out.push_str("These options work with every command, before or after the command name.\n\n");
    push_properties(&mut out, root.get_arguments().filter(|a| a.is_global_set()));

    for (path, cmd) in leaves(&root) {
        push_command(&mut out, &path, cmd.clone());
    }

    out
}

/// Every visible command that has no visible subcommands, in definition order.
fn leaves(root: &Command) -> Vec<(String, &Command)> {
    fn walk<'a>(cmd: &'a Command, prefix: &str, out: &mut Vec<(String, &'a Command)>) {
        for sub in cmd.get_subcommands().filter(|s| !s.is_hide_set() && s.get_name() != "help") {
            let path = if prefix.is_empty() { sub.get_name().to_string() } else { format!("{prefix} {}", sub.get_name()) };
            if sub.get_subcommands().any(|s| !s.is_hide_set() && s.get_name() != "help") {
                walk(sub, &path, out);
            } else {
                out.push((path, sub));
            }
        }
    }

    let mut out = Vec::new();
    walk(root, "", &mut out);
    out
}

fn push_command(out: &mut String, path: &str, mut cmd: Command) {
    let reference = REFERENCE.iter().find(|r| r.command == path);

    out.push_str(&format!("## ranksy {path}"));
    if let Some((method, endpoint)) = reference.and_then(|r| r.endpoint) {
        out.push_str(&format!(" {{{{ tag: '{method}', label: '{endpoint}' }}}}"));
    }
    out.push_str("\n\n");

    if let Some(about) = cmd.get_long_about().or(cmd.get_about()) {
        out.push_str(&sentence(&about.to_string()));
        out.push_str("\n\n");
    }

    if let Some(r) = reference {
        match r.scope {
            Some(scope) => out.push_str(&format!("- Scope: [`{scope}`](/authentication#scopes)\n")),
            None => out.push_str("- Scope: none\n"),
        }
        match r.docs {
            // The CLI's own pages are guides, not endpoint references.
            Some((title, href)) if href.starts_with("/cli/") => out.push_str(&format!("- Guide: [{title}]({href})\n")),
            Some((title, href)) => out.push_str(&format!("- API reference: [{title}]({href})\n")),
            None if r.endpoint.is_some() => out.push_str(&format!(
                "- API reference: not on this site yet. The [OpenAPI spec]({OPENAPI_URL}) describes it.\n"
            )),
            None => {}
        }
        if let Some(note) = r.note {
            out.push_str(&format!("- {note}\n"));
        }
        out.push('\n');
    }

    let usage = cmd.render_usage().to_string();
    let usage = usage.trim().trim_start_matches("Usage:").trim();
    out.push_str(&format!("```bash\n{usage}\n```\n\n"));

    let args: Vec<&Arg> = cmd
        .get_arguments()
        .filter(|a| !a.is_global_set() && !a.is_hide_set() && !matches!(a.get_id().as_str(), "help" | "version"))
        .collect();
    if !args.is_empty() {
        push_properties(out, args.into_iter());
    }
}

fn push_properties<'a>(out: &mut String, args: impl Iterator<Item = &'a Arg>) {
    out.push_str("<Properties>\n");
    for arg in args {
        let (name, kind) = arg_name(arg);
        out.push_str(&format!("  <Property name=\"{name}\" type=\"{kind}\">\n"));

        let mut text = arg.get_long_help().or(arg.get_help()).map(|h| sentence(&h.to_string())).unwrap_or_default();
        if arg.is_required_set() && arg.is_positional() {
            text.push_str(" Required.");
        }
        let defaults: Vec<String> = arg.get_default_values().iter().map(|v| v.to_string_lossy().into_owned()).collect();
        if !defaults.is_empty() && kind != "flag" {
            text.push(' ');
            text.push_str(&default_text(&defaults.join(", "), true));
        }
        if let Some(env) = arg.get_env() {
            text.push_str(&format!(" Environment variable: `{}`.", env.to_string_lossy()));
        }

        out.push_str(&format!("    {}\n", text.trim()));
        out.push_str("  </Property>\n");
    }
    out.push_str("</Properties>\n\n");
}

/// `--start-date <START_DATE>` / `option`, `--json` / `flag`, `<KEYWORD>` / `argument`.
fn arg_name(arg: &Arg) -> (String, &'static str) {
    let value = arg
        .get_value_names()
        .and_then(|names| names.first().map(|n| n.to_string()))
        .unwrap_or_else(|| arg.get_id().as_str().to_uppercase());

    if arg.is_positional() {
        return (format!("<{value}>"), "argument");
    }

    let long = arg.get_long().map(|l| format!("--{l}"));
    let short = arg.get_short().map(|s| format!("-{s}"));
    let flags = [short, long].into_iter().flatten().collect::<Vec<_>>().join(", ");
    if matches!(arg.get_action(), ArgAction::SetTrue | ArgAction::SetFalse | ArgAction::Count) {
        return (flags, "flag");
    }

    (format!("{flags} <{value}>"), "option")
}

/// Help text as MDX-safe sentences: a trailing `[default: x]` becomes `Default: x.`, it ends
/// with a period, and `<`, `{` and `}` outside backticks are escaped so MDX doesn't read them
/// as JSX.
fn sentence(text: &str) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let text = match text.rfind(" [default: ") {
        Some(i) if text.ends_with(']') => {
            let (help, value) = (&text[..i], &text[i + " [default: ".len()..text.len() - 1]);
            let literal = value.parse::<f64>().is_ok() || help.contains(&format!("`{value}`"));
            format!("{} {}", with_period(help), default_text(value, literal))
        }
        _ => text,
    };
    let escaped = text
        .split('`')
        .enumerate()
        .map(|(i, part)| {
            if i % 2 == 1 {
                return part.to_string();
            }
            part.replace('<', "&lt;").replace('{', "&#123;").replace('}', "&#125;")
        })
        .collect::<Vec<_>>()
        .join("`");

    with_period(&escaped)
}

fn with_period(text: &str) -> String {
    if text.ends_with('.') {
        return text.to_string();
    }

    format!("{text}.")
}

/// `Default: `7`.` for a literal value, `Default: 30 days ago.` for a description.
fn default_text(value: &str, literal: bool) -> String {
    if !literal {
        return format!("Default: {value}.");
    }

    format!("Default: `{value}`.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_command_has_a_reference_entry_and_every_entry_a_command() {
        let mut root = Cli::command();
        root.build();
        let paths: Vec<String> = leaves(&root).into_iter().map(|(p, _)| p).collect();

        for path in &paths {
            assert!(REFERENCE.iter().any(|r| r.command == path), "add `{path}` to REFERENCE in docs.rs");
        }
        for r in REFERENCE {
            assert!(paths.iter().any(|p| p == r.command), "REFERENCE has `{}`, which is not a command", r.command);
        }
    }

    #[test]
    fn hidden_commands_stay_out_of_the_reference() {
        assert!(!markdown().contains("ranksy docs"));
    }

    #[test]
    fn renders_endpoint_scope_usage_and_options() {
        let md = markdown();
        assert!(md.contains("## ranksy rankings get {{ tag: 'GET', label: '/api/v1/apps/:app/rankings' }}"));
        assert!(md.contains("- Scope: [`rankings:read`](/authentication#scopes)\n- API reference: [Current rankings](/rankings#current-rankings)"));
        assert!(md.contains("```bash\nranksy rankings get [OPTIONS]\n```"));
        assert!(md.contains("<Property name=\"--keyword <KEYWORD>\" type=\"option\">"));
        assert!(md.contains("<Property name=\"--chunk-days <N>\" type=\"option\">"));
        assert!(md.contains("Default: `7`."));
        assert!(md.contains("<Property name=\"--api-key <API_KEY>\" type=\"option\">"));
        assert!(md.contains("Environment variable: `RANKSY_API_KEY`."));
        assert!(md.contains("<Property name=\"-q, --quiet\" type=\"flag\">"));
        assert!(md.contains("<Property name=\"<KEY>\" type=\"argument\">"));
        assert!(md.contains("## ranksy ads disconnect\n\n"));
    }

    #[test]
    fn escapes_jsx_outside_backticks_only() {
        assert_eq!(sentence("default: <dir> or `<dir>`"), "default: &lt;dir> or `<dir>`.");
        assert_eq!(sentence("a {b}"), "a &#123;b&#125;.");
        assert_eq!(sentence("Already done."), "Already done.");
        assert_eq!(sentence("Chrome port (default 9333)"), "Chrome port (default 9333).");
    }

    #[test]
    fn moves_a_trailing_default_into_its_own_sentence() {
        assert_eq!(sentence("Rows to return, 1 to 50 [default: 20]"), "Rows to return, 1 to 50. Default: `20`.");
        assert_eq!(sentence("First day, YYYY-MM-DD [default: 30 days ago]"), "First day, YYYY-MM-DD. Default: 30 days ago.");
        assert_eq!(sentence("Last day [default: today]"), "Last day. Default: today.");
        assert_eq!(sentence("`daily` or `weekly` [default: weekly]"), "`daily` or `weekly`. Default: `weekly`.");
    }
}
