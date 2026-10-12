//! Icon inference from node ids, "did you mean" suggestions and ranked search.

use super::{IconCategory, IconEntry, SearchHit, catalog, lookup};

/// Relevance of a term matching each field (id > alias > name > category).
const SCORE_ID: f64 = 1.0;
const SCORE_ALIAS: f64 = 0.9;
const SCORE_NAME: f64 = 0.8;
const SCORE_ID_PREFIX: f64 = 0.6;
const SCORE_ALIAS_PREFIX: f64 = 0.55;
const SCORE_CATEGORY: f64 = 0.5;
const SCORE_CATEGORY_PREFIX: f64 = 0.3;
/// Weight and threshold of fuzzy (Jaro-Winkler) matches on ids and aliases.
const SCORE_FUZZY_WEIGHT: f64 = 0.4;
const FUZZY_THRESHOLD: f64 = 0.9;
/// Shortest term that may match as a prefix.
const MIN_PREFIX_LEN: usize = 2;

/// Splits a node id into lowercase words: separators, `camelCase` and letter→digit boundaries are cuts,
/// but the original word is kept too (`s3`, `k8s`, `ec2` are icon aliases).
fn words(node_id: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in node_id.split(|character: char| !character.is_ascii_alphanumeric()).filter(|raw| !raw.is_empty()) {
        let mut current = String::new();
        let mut previous: Option<char> = None;
        for character in raw.chars() {
            let camel_cut =
                previous.is_some_and(|before| before.is_ascii_lowercase() && character.is_ascii_uppercase());
            if camel_cut && !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
            current.push(character.to_ascii_lowercase());
            previous = Some(character);
        }
        if !current.is_empty() {
            out.push(current);
        }
    }
    out
}

/// Spellings of one word worth looking up: itself, without trailing digits (`api1`), singular (`dbs`).
fn spellings(word: &str) -> Vec<String> {
    let mut out = vec![word.to_owned()];
    let without_digits = word.trim_end_matches(|character: char| character.is_ascii_digit());
    if !without_digits.is_empty() && without_digits != word {
        out.push(without_digits.to_owned());
    }
    let base = out[out.len() - 1].clone();
    if base.len() > 3 && base.ends_with('s') && !base.ends_with("ss") {
        out.push(base[..base.len() - 1].to_owned());
    }
    out
}

fn lookup_any(spellings: &[String]) -> Option<&'static IconEntry> {
    spellings.iter().find_map(|spelling| lookup(spelling))
}

/// Infers an icon from a node id when `icon` is omitted (`api1` → server, `orders_db` → database,
/// `user_cache` → cache, `load_balancer` → loadbalancer). The whole id wins, then word pairs, then
/// single words from the last one (the head of `orders_db` is `db`). `None` → the caller uses `block`.
pub fn infer_from_node_id(node_id: &str) -> Option<&'static IconEntry> {
    let words = words(node_id);
    if words.is_empty() {
        return None;
    }
    let whole = words.join("-");
    if let Some(entry) = lookup_any(&spellings(&whole)) {
        return Some(entry);
    }
    for pair in words.windows(2).rev() {
        let joined = [format!("{}-{}", pair[0], pair[1]), format!("{}{}", pair[0], pair[1])];
        if let Some(entry) = joined.iter().find_map(|spelling| lookup_any(&spellings(spelling))) {
            return Some(entry);
        }
    }
    words.iter().rev().find_map(|word| lookup_any(&spellings(word)))
}

/// Every id and alias, in catalog order (ids first within an entry).
fn names() -> impl Iterator<Item = &'static str> {
    catalog().iter().flat_map(|entry| std::iter::once(entry.id).chain(entry.aliases.iter().copied()))
}

/// "Did you mean" for an unknown icon (`postgress` → `postgres`): the closest id or alias by
/// Damerau-Levenshtein distance, if within a third of the word's length (at least 1 edit).
pub fn suggest(unknown: &str) -> Option<&'static str> {
    let wanted = unknown.trim().to_ascii_lowercase();
    if wanted.is_empty() {
        return None;
    }
    let budget = (wanted.chars().count() / 3).max(1);
    let mut best: Option<(usize, &'static str)> = None;
    for name in names() {
        let distance = strsim::damerau_levenshtein(&wanted, name);
        if distance <= budget && best.is_none_or(|(closest, _)| distance < closest) {
            best = Some((distance, name));
        }
    }
    best.map(|(_, name)| name)
}

/// The part after the pack of a flat id (`aws/lambda` → `lambda`); built-in names are returned unchanged.
fn local_name(name: &str) -> &str {
    name.split_once('/').map_or(name, |(_, local)| local)
}

/// Score of one query term against an entry (0 when unrelated). Flat icons also match by their name inside
/// the pack (`lambda` for `aws/lambda`) and by the pack itself (`aws`, scored like a category).
fn term_score(entry: &IconEntry, term: &str) -> f64 {
    let name = entry.name.to_ascii_lowercase();
    let category = entry.category.as_str();
    let pack = super::flat::entry_pack(entry);
    let local_id = local_name(entry.id);
    let local_alias = |alias: &&str| local_name(alias) == term;
    let exact = if entry.id == term || (pack.is_some() && local_id == term) {
        SCORE_ID
    } else if entry.aliases.contains(&term) || (pack.is_some() && entry.aliases.iter().any(local_alias)) {
        SCORE_ALIAS
    } else if name == term || name.split(' ').any(|word| word == term) {
        SCORE_NAME
    } else {
        0.0
    };
    if exact > 0.0 {
        return exact;
    }
    let long_enough = term.len() >= MIN_PREFIX_LEN;
    let prefix = if long_enough && (entry.id.starts_with(term) || (pack.is_some() && local_id.starts_with(term))) {
        SCORE_ID_PREFIX
    } else if long_enough && entry.aliases.iter().any(|alias| local_name(alias).starts_with(term)) {
        SCORE_ALIAS_PREFIX
    } else if category == term || pack == Some(term) {
        SCORE_CATEGORY
    } else if long_enough && category.starts_with(term) {
        SCORE_CATEGORY_PREFIX
    } else {
        0.0
    };
    let fuzzy = std::iter::once(local_id)
        .chain(entry.aliases.iter().map(|alias| local_name(alias)))
        .map(|candidate| strsim::jaro_winkler(term, candidate))
        .fold(0.0, f64::max);
    let fuzzy = if fuzzy >= FUZZY_THRESHOLD { SCORE_FUZZY_WEIGHT * fuzzy } else { 0.0 };
    prefix.max(fuzzy)
}

/// Score of a query against an entry: the whole query as one name (`load balancer`), or the mean of
/// its terms (`message queue`).
fn query_score(entry: &IconEntry, terms: &[String]) -> f64 {
    let whole = [terms.join("-"), terms.join(" ")].iter().map(|joined| term_score(entry, joined)).fold(0.0, f64::max);
    let mean = terms.iter().map(|term| term_score(entry, term)).sum::<f64>() / terms.len() as f64;
    whole.max(mean)
}

/// Ranked search over id, name, aliases and category (`"message queue"`, `"aws lambda"`).
/// An empty query lists the catalog (score 1). Ties keep catalog order.
pub fn search(query: &str, category: Option<IconCategory>, limit: usize) -> Vec<SearchHit> {
    let entries = catalog().iter().filter(|entry| category.is_none_or(|wanted| entry.category == wanted));
    let mut hits = rank(query, entries);
    hits.truncate(limit);
    hits
}

/// Scores `entries` against `query` and keeps the matches, best first (stable: ties keep input order).
pub(super) fn rank(query: &str, entries: impl Iterator<Item = &'static IconEntry>) -> Vec<SearchHit> {
    let terms: Vec<String> = query
        .split(|character: char| character.is_whitespace() || character == ',' || character == '_')
        .filter(|term| !term.is_empty())
        .map(str::to_ascii_lowercase)
        .collect();
    let mut hits: Vec<SearchHit> = entries
        .map(|entry| SearchHit { entry, score: if terms.is_empty() { 1.0 } else { query_score(entry, &terms) } })
        .filter(|hit| hit.score > 0.0)
        .collect();
    hits.sort_by(|a, b| b.score.total_cmp(&a.score));
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inferred(node_id: &str) -> Option<&'static str> {
        infer_from_node_id(node_id).map(|entry| entry.id)
    }

    #[test]
    fn infers_from_node_ids() {
        for (node_id, icon) in [
            ("api1", Some("server")),
            ("api", Some("server")),
            ("orders_db", Some("database")),
            ("db", Some("database")),
            ("lb", Some("loadbalancer")),
            ("user_cache", Some("cache")),
            ("load_balancer", Some("loadbalancer")),
            ("api_gateway", Some("api-gateway")),
            ("apiGateway", Some("api-gateway")),
            ("users", Some("users")),
            ("s3", Some("storage")),
            ("backup_nas", Some("nas")),
            ("disk", Some("nas")),
            ("k8s", Some("k8s-cluster")),
            ("primary-postgres-2", Some("database")),
            ("redisCache", Some("cache")),
            ("orders_queue", Some("queue")),
            ("servers", Some("server")),
            ("payments", None),
            ("", None),
            ("x_9", None),
        ] {
            assert_eq!(inferred(node_id), icon, "{node_id}");
        }
    }

    #[test]
    fn suggests_close_names() {
        assert_eq!(suggest("postgress"), Some("postgres"));
        assert_eq!(suggest("Postgress"), Some("postgres"));
        assert_eq!(suggest("databse"), Some("database"));
        assert_eq!(suggest("redsi"), Some("redis"));
        assert_eq!(suggest("kubernets"), Some("kubernetes"));
        assert_eq!(suggest("zzzzzzzz"), None);
        assert_eq!(suggest(""), None);
    }

    #[test]
    fn search_ranks_relevant_icons_first() {
        let top = |query: &str| search(query, None, 5).first().map(|hit| hit.entry.id);
        assert_eq!(top("message queue"), Some("queue"));
        assert_eq!(top("aws lambda"), Some("function"));
        assert_eq!(top("load balancer"), Some("loadbalancer"));
        assert_eq!(top("postgres"), Some("database"));
        assert_eq!(top("Redis"), Some("cache"));
        assert_eq!(top("k8s"), Some("k8s-cluster"));
        assert_eq!(top("monitor"), Some("monitoring"));
        assert_eq!(top("nas"), Some("nas"));
        assert_eq!(top("san"), Some("nas"));
        assert_eq!(top("s3"), Some("storage"));
    }

    #[test]
    fn search_prefers_id_over_alias_over_name_over_category() {
        // `server` is an id; `server-rack` is an alias of rack; both are prefixes for "serv".
        let hits = search("server", None, 10);
        assert_eq!(hits[0].entry.id, "server");
        assert!(hits.iter().all(|hit| hit.score <= 1.0 && hit.score > 0.0));
        let by_category = search("network", None, 20);
        assert!(by_category.iter().all(|hit| hit.entry.category == IconCategory::Network));
        assert!(by_category.len() >= 7);
    }

    #[test]
    fn search_filters_and_limits() {
        let data = search("", Some(IconCategory::Data), 10);
        let ids: Vec<&str> = data.iter().map(|hit| hit.entry.id).collect();
        assert_eq!(ids, ["database", "cache", "storage", "nas"]);
        assert_eq!(search("", None, 3).len(), 3);
        assert_eq!(search("", None, 100).len(), catalog().len());
        assert!(search("cache", Some(IconCategory::Network), 10).is_empty());
        assert!(search("qwertyuiop", None, 10).is_empty());
    }

    #[test]
    fn aliases_never_collide() {
        let mut seen = std::collections::BTreeMap::new();
        for entry in catalog() {
            for name in std::iter::once(entry.id).chain(entry.aliases.iter().copied()) {
                assert_eq!(name, name.to_ascii_lowercase(), "{name} must be lowercase");
                if let Some(previous) = seen.insert(name, entry.id) {
                    panic!("'{name}' belongs to both '{previous}' and '{}'", entry.id);
                }
            }
        }
    }
}
