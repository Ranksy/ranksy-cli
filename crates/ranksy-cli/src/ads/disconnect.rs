//! `ranksy ads disconnect`: stop syncing one Partners organization.

use super::{describe_orgs, Ctx, NOT_CONNECTED};
use crate::config::{self, AdsConfig};
use anyhow::{anyhow, bail, Result};

pub fn run(ctx: &Ctx, org: &str) -> Result<()> {
    let ads = ctx
        .file
        .ads
        .as_ref()
        .ok_or_else(|| anyhow!(NOT_CONNECTED))?;
    let (next, removed) = remove_org(ads, org)?;
    config::save_ads(next.clone()).map_err(|e| anyhow!(e.to_string()))?;

    println!("Disconnected Partners organization {org}.");
    for app in &removed {
        println!("  removed mapping for {app}");
    }
    println!("Connected organizations: {}.", describe_orgs(&next.orgs));

    Ok(())
}

/// The `[ads]` table without `org` and the mappings that belong to it, plus the names of the
/// removed mappings. Legacy mappings (no org) belong to the only org when there is just one.
pub fn remove_org(ads: &AdsConfig, org: &str) -> Result<(AdsConfig, Vec<String>)> {
    if !ads.orgs.iter().any(|o| o == org) {
        bail!(
            "Partners organization {org} is not connected (connected: {}).",
            describe_orgs(&ads.orgs)
        );
    }

    let only_org = ads.orgs.len() == 1;
    let mut next = ads.clone();
    next.orgs.retain(|o| o != org);
    let mut removed = Vec::new();
    next.apps.retain(|key, app| {
        let belongs = match app.org.as_deref() {
            Some(o) => o == org,
            None => only_org,
        };
        if belongs {
            removed.push(app.name.clone().unwrap_or_else(|| key.clone()));
        }

        !belongs
    });

    Ok((next, removed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AdsApp;
    use std::collections::BTreeMap;

    fn app(ulid: &str, name: Option<&str>, org: Option<&str>) -> AdsApp {
        AdsApp {
            ulid: ulid.into(),
            name: name.map(str::to_string),
            org: org.map(str::to_string),
        }
    }

    fn two_orgs() -> AdsConfig {
        AdsConfig {
            orgs: vec!["1".into(), "2".into()],
            apps: BTreeMap::from([
                ("k1".to_string(), app("01A", Some("One"), Some("1"))),
                ("k2".to_string(), app("01B", None, Some("2"))),
                ("k3".to_string(), app("01C", Some("Three"), Some("2"))),
            ]),
            ..Default::default()
        }
    }

    #[test]
    fn removes_the_org_and_only_its_apps() {
        let (next, removed) = remove_org(&two_orgs(), "2").unwrap();
        assert_eq!(next.orgs, ["1"]);
        assert_eq!(next.apps.keys().collect::<Vec<_>>(), ["k1"]);
        assert_eq!(removed, ["k2", "Three"]);
    }

    #[test]
    fn unknown_org_errors_and_lists_the_connected_ones() {
        let err = remove_org(&two_orgs(), "9").unwrap_err().to_string();
        assert!(err.contains("9") && err.contains("1, 2"));
    }

    #[test]
    fn legacy_mappings_go_with_the_only_org_but_survive_otherwise() {
        let mut ads = AdsConfig {
            orgs: vec!["1".into()],
            apps: BTreeMap::from([("old".to_string(), app("01O", None, None))]),
            ..Default::default()
        };
        let (next, removed) = remove_org(&ads, "1").unwrap();
        assert!(next.orgs.is_empty() && next.apps.is_empty());
        assert_eq!(removed, ["old"]);

        ads.orgs.push("2".into());
        let (next, removed) = remove_org(&ads, "2").unwrap();
        assert_eq!(next.apps.len(), 1);
        assert!(removed.is_empty());
    }
}
