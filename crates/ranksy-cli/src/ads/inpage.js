// Functions that run INSIDE the logged-in Shopify Partners page (via CDP Runtime.evaluate).
// Ported from scripts/shopify-ads-export.mjs in the Ranksy app with minimal change.
// The Rust side prepends this file to every evaluation, then calls one function:
//   await inPageSnapshot(inPageGql(org), org)
//   await inPagePullDay(inPageGql(org), org, day)
// Shopify cookies and the CSRF token never leave the browser.

// Builds the in-page GraphQL client (cookies + CSRF token of the logged-in page).
function inPageGql(org) {
  const csrf = document.querySelector('meta[name="csrf-token"]')?.content;

  return async (op, query, variables = {}) => {
    for (let attempt = 1; ; attempt++) {
      const r = await fetch(`/${org}/api/ad_platform_graphql?operation=${op}`, {
        method: 'POST',
        credentials: 'include',
        headers: { 'content-type': 'application/json', accept: 'application/json', 'x-csrf-token': csrf },
        body: JSON.stringify({ operationName: op, query, variables }),
      });
      const j = await r.json().catch(() => null);
      // Partners rate limits answer 400 {"error":"Too many requests for operation"}, not 429.
      const limited = r.status === 429 || /too many requests/i.test(String(j?.error ?? ''));
      if ((limited || r.status >= 500) && attempt < 8) {
        await new Promise((res) => setTimeout(res, 5000 * attempt));
        continue;
      }
      if (!r.ok || !j?.data) {
        throw new Error(`${op} HTTP ${r.status} ${JSON.stringify(j?.errors ?? j).slice(0, 300)}`);
      }

      return j.data;
    }
  };
}

// `ranksy ads sync --only-app`: `only` is null (every app) or the config keys of the selected
// apps, which are Partners app API keys (or app names for older mappings).
function inPageKeeps(only, c) {
  return !only || only.includes(c.app?.apiKey) || only.includes(c.app?.name);
}

// Organizations linked from the Partners organization picker (/organizations).
async function inPageListOrgs() {
  const orgs = new Map();
  for (const a of document.querySelectorAll('a[href]')) {
    const m = a.getAttribute('href').match(/^(?:https:\/\/partners\.shopify\.com)?\/(\d+)(?:[\/?#]|$)/);
    if (m && !orgs.has(m[1])) {
      orgs.set(m[1], (a.textContent ?? '').replace(/\s+/g, ' ').trim().slice(0, 80));
    }
  }

  return { orgs: [...orgs].map(([id, name]) => ({ id, name })) };
}

// Shopify apps that have ad campaigns (active or archived); used by `ranksy ads connect`.
async function inPageApps(gql) {
  const apps = new Map();
  for (const archived of [false, true]) {
    let after = null;
    do {
      const d = await gql('RanksyApps', `query RanksyApps($a: Boolean, $after: String) { campaigns(first: 100, archived: $a, after: $after) { pageInfo { hasNextPage endCursor } nodes { id app { apiKey name } } } }`, { a: archived, after });
      for (const c of d.campaigns.nodes) {
        if (c.app?.apiKey && !apps.has(c.app.apiKey)) {
          apps.set(c.app.apiKey, { apiKey: c.app.apiKey, name: c.app.name, campaigns: 0 });
        }
        if (c.app?.apiKey) {
          apps.get(c.app.apiKey).campaigns++;
        }
      }
      after = d.campaigns.pageInfo.hasNextPage ? d.campaigns.pageInfo.endCursor : null;
    } while (after);
  }

  return { apps: [...apps.values()] };
}

// Campaign metadata + keyword / negative keyword snapshots (current state). With `only`, the
// per-campaign and per-app queries below run for the selected apps only (the campaign list
// itself is organization-wide).
async function inPageSnapshot(gql, _org, only = null) {
  const META = `__typename id name status surface subsurface dailyBudget createdAt startingAt endingAt app { apiKey name }
    targeting { geographicTargets { code } planTargets deviceTypeTargets }
    ... on DisplayCampaign { bid suggestedBidRange { lower upper } }`;
  let campaigns = [];
  for (const archived of [false, true]) {
    let after = null;
    do {
      const d = await gql('RanksyCampaigns', `query RanksyCampaigns($a: Boolean, $after: String) { campaigns(first: 100, archived: $a, after: $after) { pageInfo { hasNextPage endCursor } nodes { ${META} } } }`, { a: archived, after });
      campaigns.push(...d.campaigns.nodes.map((c) => ({ ...c, archived })));
      after = d.campaigns.pageInfo.hasNextPage ? d.campaigns.pageInfo.endCursor : null;
    } while (after);
  }
  campaigns = campaigns.filter((c) => inPageKeeps(only, c));

  for (const c of campaigns.filter((c) => c.__typename === 'SearchCampaign')) {
    c.keywords = [];
    for (const archived of [false, true]) {
      let after = null;
      do {
        const d = await gql('RanksyKeywords', `query RanksyKeywords($id: ID!, $a: Boolean, $after: String) { campaign(id: $id) { ... on SearchCampaign {
          keywordBidCriteria(first: 100, archived: $a, after: $after) { pageInfo { hasNextPage endCursor } nodes { id keyword matchType bid status relevanceCategory suggestedBidRange { lower upper } } } } } }`, { id: c.id, a: archived, after });
        const k = d.campaign.keywordBidCriteria;
        c.keywords.push(...k.nodes);
        after = k.pageInfo.hasNextPage ? k.pageInfo.endCursor : null;
      } while (after);
    }
    c.negativeKeywords = [];
    let after = null;
    do {
      const d = await gql('RanksyNegatives', `query RanksyNegatives($id: ID!, $after: String) { campaign(id: $id) { ... on SearchCampaign {
        negativeKeywords(first: 100, after: $after) { pageInfo { hasNextPage endCursor } nodes { id keyword matchType } } } } }`, { id: c.id, after });
      const n = d.campaign.negativeKeywords;
      c.negativeKeywords.push(...n.nodes);
      after = n.pageInfo.hasNextPage ? n.pageInfo.endCursor : null;
    } while (after);

    // Shopify's own suggestions for live campaigns; optional, so a failure never blocks the sync.
    if (!c.archived) {
      try {
        const d = await gql('RanksyRecommendations', `query RanksyRecommendations($id: ID!) { recommendations(campaignId: $id, dismissed: false) {
          newKeywords { id keyword searchTerm bid returnOnAdSpend } pauseKeywords { id keyword matchType returnOnAdSpend } } }`, { id: c.id });
        c.recommendations = d.recommendations;
      } catch {
        c.recommendations = null;
      }
    }
  }

  // Shopify's keyword ideas per app (plain strings), also optional.
  const keywordIdeas = {};
  for (const apiKey of new Set(campaigns.map((c) => c.app?.apiKey).filter(Boolean))) {
    try {
      const d = await gql('RanksyKeywordIdeas', `query RanksyKeywordIdeas($k: String!) { keywordRecommendations(apiKey: $k) }`, { k: apiKey });
      keywordIdeas[apiKey] = d.keywordRecommendations ?? [];
    } catch {
      // Not every account gets keyword ideas.
    }
  }

  return { campaigns, keywordIdeas };
}

// One day: campaign metrics, breakdowns, and search terms with impressions. With `only`, the
// campaigns of other apps are dropped right after the (organization-wide) day listing, so
// their breakdown and search-term queries never run.
async function inPagePullDay(gql, org, day, only = null) {
  const M = 'impressions clicks clickthroughRate installs installRate costPerClick costPerInstall costPerMille spend averagePosition page1WinRate customers revenue returnOnSpend costPerCustomer customerRate conversions conversionRate costPerConversion';
  let campaigns = [];
  for (const archived of [false, true]) {
    let after = null;
    do {
      const d = await gql('RanksyDayCampaigns', `query RanksyDayCampaigns($s: Date!, $e: Date!, $a: Boolean, $after: String) {
        campaignsWithMetrics(startDate: $s, endDate: $e, first: 100, archived: $a, after: $after) {
          pageInfo { hasNextPage endCursor } nodes { campaign { id surface app { apiKey name } } metrics { ${M} } } } }`, { s: day, e: day, a: archived, after });
      campaigns.push(...d.campaignsWithMetrics.nodes.map((n) => ({ id: n.campaign.id, surface: n.campaign.surface, archived, app: n.campaign.app, metrics: n.metrics })));
      after = d.campaignsWithMetrics.pageInfo.hasNextPage ? d.campaignsWithMetrics.pageInfo.endCursor : null;
    } while (after);
  }
  campaigns = campaigns.filter((c) => inPageKeeps(only, c));

  for (const c of campaigns) {
    if (c.metrics.impressions === 0) {
      continue;
    }
    c.breakdowns = {};
    for (const target of ['country', 'shop_plan', 'device_type']) {
      const d = await gql('RanksyDayBreakdown', `query RanksyDayBreakdown($ids: [ID!], $s: Date!, $e: Date!, $t: Target!, $a: Boolean) {
        campaignsMetricsByTarget(campaignIds: $ids, startDate: $s, endDate: $e, target: $t, archived: $a) { metrics { ${M} } targeting { geographicTargets { code } planTargets deviceTypeTargets } } }`, { ids: [c.id], s: day, e: day, t: target, a: c.archived });
      c.breakdowns[target] = d.campaignsMetricsByTarget;
    }
    if (c.surface !== 'appstore_search') {
      continue;
    }
    // Sorted by impressions: stop at the first zero-impression term (~90% of rows are zeros).
    c.searchTerms = [];
    let after = null;
    do {
      const d = await gql('RanksyDayTerms', `query RanksyDayTerms($id: ID!, $s: Date!, $e: Date!, $after: String) { campaign(id: $id) { ... on SearchCampaign {
        searchTerms(startDate: $s, endDate: $e, first: 100, after: $after, sortBy: { field: impressions, direction: Desc }) {
          pageInfo { hasNextPage endCursor } nodes { term keywordBidCriterion { id keyword matchType bid } metrics { ${M} } } } } } }`, { id: c.id, s: day, e: day, after });
      const st = d.campaign.searchTerms;
      const withImpressions = st.nodes.filter((t) => t.metrics?.impressions > 0);
      c.searchTerms.push(...withImpressions);
      after = st.pageInfo.hasNextPage && withImpressions.length === st.nodes.length ? st.pageInfo.endCursor : null;
    } while (after);
  }

  return { campaigns };
}
