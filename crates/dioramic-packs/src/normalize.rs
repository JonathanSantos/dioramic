//! Provider file names → icon ids (`Arch_AWS-Lambda_64.svg` → `lambda`, `10029-icon-service-Function-Apps.svg`
//! → `function-apps` with alias `functions`). One rule set per provider; every rule is a pure function of the
//! file's path inside the archive, so installs are reproducible.

use std::collections::BTreeMap;

use dioramic_core::icons::Mount;

use crate::provider::Provider;

/// An SVG found in a provider archive or directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFile {
    /// Path inside the archive/directory, `/`-separated.
    pub path: String,
    /// File contents.
    pub svg: String,
}

/// An icon picked from the sources, ready for the manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedIcon {
    /// Id inside the pack (`lambda`).
    pub id: String,
    /// Display name (`AWS Lambda`).
    pub name: String,
    /// Other ids (`aws-lambda`).
    pub aliases: Vec<String>,
    /// Provider category (`Compute`).
    pub group: Option<String>,
    /// Mount.
    pub mount: Mount,
    /// Index of the chosen file in the input.
    pub file: usize,
}

/// A candidate before de-duplication: lower `rank` wins when two files map to the same id.
struct Candidate {
    id: String,
    name: String,
    aliases: Vec<String>,
    group: Option<String>,
    rank: u32,
    file: usize,
}

/// Picks one file per icon id, sorted by id. Files that match no rule of the provider are skipped.
pub fn normalize(provider: Provider, files: &[SourceFile]) -> Vec<NormalizedIcon> {
    let mut best: BTreeMap<String, Candidate> = BTreeMap::new();
    for (index, file) in files.iter().enumerate() {
        let Some(mut candidate) = candidate(provider, &file.path) else { continue };
        candidate.file = index;
        let keep_existing = best
            .get(&candidate.id)
            .is_some_and(|existing| (existing.rank, &files[existing.file].path) <= (candidate.rank, &file.path));
        if !keep_existing {
            best.insert(candidate.id.clone(), candidate);
        }
    }
    let taken: Vec<String> = best.keys().cloned().collect();
    best.into_values()
        .map(|candidate| {
            let mut aliases: Vec<String> = candidate
                .aliases
                .into_iter()
                .filter(|alias| alias != &candidate.id && taken.binary_search(alias).is_err() && is_valid_id(alias))
                .collect();
            aliases.sort();
            aliases.dedup();
            NormalizedIcon {
                id: candidate.id,
                name: candidate.name,
                aliases,
                group: candidate.group,
                mount: Mount::Top,
                file: candidate.file,
            }
        })
        .collect()
}

fn candidate(provider: Provider, path: &str) -> Option<Candidate> {
    let path = path.trim_start_matches("./");
    let segments: Vec<&str> = path.split('/').filter(|segment| !segment.is_empty()).collect();
    let file_name = segments.last()?;
    if segments.contains(&"__MACOSX")
        || file_name.starts_with("._")
        || !file_name.to_ascii_lowercase().ends_with(".svg")
    {
        return None;
    }
    let stem = &file_name[..file_name.len() - ".svg".len()];
    match provider {
        Provider::Aws => aws(&segments, stem),
        Provider::Gcp => gcp(&segments, stem),
        Provider::Azure => azure(&segments, stem),
        Provider::Kubernetes => kubernetes(&segments, stem),
    }
}

/// `Some Words_Here` → `some-words-here` (only `[a-z0-9-]`, no repeated or edge dashes).
pub fn slug(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        if character.is_ascii_alphanumeric() {
            out.push(character.to_ascii_lowercase());
        } else if character == '&' {
            out.push_str("-and-");
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let mut collapsed = String::with_capacity(out.len());
    for character in out.chars() {
        if !(character == '-' && collapsed.ends_with('-')) {
            collapsed.push(character);
        }
    }
    collapsed.trim_matches('-').to_owned()
}

fn is_valid_id(id: &str) -> bool {
    !id.is_empty() && id == slug(id)
}

/// `AWS-Lambda` → `AWS Lambda`.
fn display_name(text: &str) -> String {
    text.replace(['-', '_'], " ").split_whitespace().collect::<Vec<_>>().join(" ")
}

fn strip_prefix_word<'a>(id: &'a str, prefixes: &[&str]) -> &'a str {
    prefixes.iter().find_map(|prefix| id.strip_prefix(prefix).filter(|rest| !rest.is_empty())).unwrap_or(id)
}

// ---------------------------------------------------------------------------------------------------------------
// AWS: Architecture-Service-Icons_<date>/Arch_<Group>/<size>/Arch_<Service>_<size>.svg and
//      Resource-Icons_<date>/Res_<Group>/Res_<Service>_<Resource>_48.svg

/// Size folders of the service icons, best first (64 px is the reference artwork).
const AWS_SIZES: [&str; 4] = ["64", "48", "32", "16"];

/// Short names people use for AWS services (`aws/s3`), by service id.
const AWS_ALIASES: &[(&str, &[&str])] = &[
    ("simple-storage-service", &["s3", "bucket"]),
    ("simple-storage-service-glacier", &["glacier", "s3-glacier"]),
    ("simple-queue-service", &["sqs", "queue"]),
    ("simple-notification-service", &["sns"]),
    ("simple-email-service", &["ses"]),
    ("elastic-kubernetes-service", &["eks"]),
    ("elastic-container-service", &["ecs"]),
    ("elastic-container-registry", &["ecr"]),
    ("elastic-block-store", &["ebs"]),
    ("efs", &["elastic-file-system"]),
    ("rds", &["relational-database-service"]),
    ("elasticache", &["redis", "memcached"]),
    ("api-gateway", &["apigateway", "apigw"]),
    ("route-53", &["route53", "dns"]),
    ("virtual-private-cloud", &["vpc"]),
    ("elastic-load-balancing", &["elb", "alb", "nlb", "load-balancer"]),
    ("identity-and-access-management", &["iam"]),
    ("key-management-service", &["kms"]),
    ("managed-streaming-for-apache-kafka", &["msk", "kafka"]),
    ("opensearch-service", &["opensearch", "elasticsearch"]),
    ("lambda", &["function", "functions"]),
    ("ec2", &["instance", "vm"]),
    ("ec2-auto-scaling", &["auto-scaling-group", "asg"]),
    ("cloudwatch", &["monitoring"]),
    ("waf", &["firewall"]),
    ("step-functions", &["stepfunctions", "sfn"]),
    ("kinesis-data-streams", &["data-streams"]),
    ("data-firehose", &["firehose", "kinesis-firehose"]),
    ("certificate-manager", &["acm"]),
    ("systems-manager", &["ssm"]),
];

fn aws(segments: &[&str], stem: &str) -> Option<Candidate> {
    let parent = segments.len().checked_sub(2).map(|index| segments[index])?;
    if let Some(service) = stem.strip_prefix("Arch_") {
        let size_rank = AWS_SIZES.iter().position(|size| *size == parent)? as u32;
        let service = strip_aws_size(service)?;
        let group = segments.iter().find_map(|segment| segment.strip_prefix("Arch_")).map(display_name);
        return aws_candidate(service, group, size_rank);
    }
    if let Some(resource) = stem.strip_prefix("Res_") {
        let resource = strip_aws_size(resource)?;
        let group = parent.strip_prefix("Res_").map(display_name);
        // Resource icons come after every service icon on an id clash.
        return aws_candidate(resource, group, AWS_SIZES.len() as u32 + 1);
    }
    None
}

/// `AWS-Lambda_64` → `AWS-Lambda`; `AWS-Marketplace_Light_64` → `AWS-Marketplace`; dark variants are skipped.
fn strip_aws_size(name: &str) -> Option<&str> {
    let name = name.trim_end_matches("@5x");
    let name =
        name.rsplit_once('_').filter(|(_, size)| size.chars().all(|c| c.is_ascii_digit())).map_or(name, |(n, _)| n);
    if name.ends_with("_Dark") {
        return None;
    }
    Some(name.trim_end_matches("_Light"))
}

fn aws_candidate(name: &str, group: Option<String>, rank: u32) -> Option<Candidate> {
    let full = slug(name);
    let id = strip_prefix_word(&full, &["aws-", "amazon-"]).to_owned();
    if id.is_empty() {
        return None;
    }
    let mut aliases = vec![full.clone()];
    for (service, short_names) in AWS_ALIASES {
        if id == *service {
            aliases.extend(short_names.iter().map(|alias| (*alias).to_owned()));
        } else if let Some(rest) = id.strip_prefix(&format!("{service}-")) {
            // Resource icons get only the main short name (`s3-bucket`, not `bucket-bucket`).
            aliases.extend(short_names.first().map(|alias| format!("{alias}-{rest}")));
        }
    }
    Some(Candidate { id, name: display_name(name), aliases, group, rank, file: 0 })
}

// ---------------------------------------------------------------------------------------------------------------
// Google Cloud: core-products-icons.zip (Unique Icons/<Product>/SVG/<Name>-512-color.svg), category-icons.zip
// (Category Icons/<Category>/SVG/…) and google-cloud-legacy-icons.zip (<product>/<product>.svg).

/// Short names of Google Cloud products, by id.
const GCP_ALIASES: &[(&str, &[&str])] = &[
    ("cloud-storage", &["gcs", "storage", "bucket"]),
    ("compute-engine", &["gce", "vm"]),
    ("google-kubernetes-engine", &["kubernetes-engine"]),
    ("gke", &["google-kubernetes-engine", "kubernetes-engine"]),
    ("pubsub", &["pub-sub", "cloud-pubsub"]),
    ("cloud-functions", &["functions", "function"]),
    ("cloud-run", &["run"]),
    ("cloud-sql", &["sql"]),
    ("cloud-spanner", &["spanner"]),
    ("memorystore", &["redis"]),
    ("cloud-load-balancing", &["load-balancer", "lb"]),
    ("cloud-cdn", &["cdn"]),
    ("cloud-dns", &["dns"]),
    ("cloud-armor", &["waf"]),
    ("vertex-ai", &["vertexai"]),
    ("cloud-monitoring", &["monitoring"]),
    ("cloud-logging", &["logging"]),
    ("identity-and-access-management", &["iam"]),
    ("virtual-private-cloud", &["vpc"]),
    ("cloud-api-gateway", &["api-gateway"]),
];

/// Coarse groups of Google Cloud products (the archives have no categories), by id keyword.
const GCP_GROUPS: &[(&str, &[&str])] = &[
    ("Databases", &["sql", "spanner", "bigtable", "firestore", "datastore", "memorystore", "alloydb", "database"]),
    ("Storage", &["storage", "filestore", "disk", "hyperdisk", "transfer"]),
    ("Analytics", &["bigquery", "dataflow", "dataproc", "pubsub", "composer", "looker", "data"]),
    ("Compute", &["compute", "run", "functions", "app-engine", "gke", "kubernetes", "batch", "tpu", "gpu"]),
    (
        "Networking",
        &["network", "vpc", "dns", "cdn", "load-balancing", "nat", "vpn", "interconnect", "router", "armor"],
    ),
    ("Security", &["security", "iam", "key", "secret", "identity", "certificate", "firewall"]),
    ("Operations", &["monitoring", "logging", "trace", "debugger", "profiler", "ops", "error"]),
];

fn gcp(segments: &[&str], stem: &str) -> Option<Candidate> {
    if let Some(position) = segments.iter().position(|segment| *segment == "SVG") {
        let product = position.checked_sub(1).map(|index| segments[index])?;
        let is_category = segments.iter().any(|segment| segment.eq_ignore_ascii_case("Category Icons"));
        let base = slug(product);
        let id = if is_category { format!("category-{base}") } else { base };
        let name = if is_category { format!("{} (category)", display_name(product)) } else { display_name(product) };
        // Prefer the RGB variant when both exist.
        let rank = if stem.ends_with("-rgb") { 0 } else { 1 };
        return Some(gcp_candidate(id, name, rank));
    }
    // Legacy layout: <product>/<product>.svg.
    let parent = segments.len().checked_sub(2).map(|index| segments[index])?;
    if parent != stem {
        return None;
    }
    Some(gcp_candidate(slug(stem), title_case(stem), 2))
}

/// Words that stay upper case in legacy Google Cloud names (`cloud_sql` → `Cloud SQL`).
const ACRONYMS: &[&str] = &[
    "ai", "api", "apis", "cdn", "dns", "gke", "gpu", "hsm", "iam", "ids", "iot", "nat", "nlp", "os", "sql", "ssd",
    "tpu", "vpc", "vpn",
];

/// `cloud_sql` → `Cloud SQL`.
fn title_case(stem: &str) -> String {
    display_name(stem)
        .split(' ')
        .map(|word| {
            if ACRONYMS.contains(&word.to_ascii_lowercase().as_str()) {
                word.to_ascii_uppercase()
            } else {
                let mut characters = word.chars();
                characters.next().map_or_else(String::new, |first| first.to_uppercase().chain(characters).collect())
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn gcp_candidate(id: String, name: String, rank: u32) -> Candidate {
    let mut aliases = Vec::new();
    if let Some(short) = id.strip_prefix("cloud-") {
        aliases.push(short.to_owned());
    }
    for (product, short_names) in GCP_ALIASES {
        if id == *product {
            aliases.extend(short_names.iter().map(|alias| (*alias).to_owned()));
        }
    }
    let group = GCP_GROUPS
        .iter()
        .find(|(_, keywords)| keywords.iter().any(|keyword| id.contains(keyword)))
        .map(|(group, _)| (*group).to_owned());
    Candidate { id, name, aliases, group, rank, file: 0 }
}

// ---------------------------------------------------------------------------------------------------------------
// Azure: Azure_Public_Service_Icons/Icons/<category>/<NNNNN>-icon-service-<Name>.svg

/// Short names of Azure services, by id.
const AZURE_ALIASES: &[(&str, &[&str])] = &[
    ("function-apps", &["functions", "function", "function-app"]),
    ("kubernetes-services", &["aks", "kubernetes-service"]),
    ("cosmos-db", &["cosmosdb", "cosmos"]),
    ("sql-database", &["sql", "sql-db"]),
    ("app-services", &["app-service", "web-app"]),
    ("storage-accounts", &["storage", "storage-account", "blob"]),
    ("key-vaults", &["key-vault", "keyvault"]),
    ("api-management-services", &["api-management", "apim"]),
    ("service-bus", &["servicebus"]),
    ("event-hubs", &["event-hub", "eventhubs"]),
    ("virtual-machine", &["vm"]),
    ("virtual-networks", &["vnet", "virtual-network"]),
    ("load-balancers", &["load-balancer", "lb"]),
    ("application-gateways", &["application-gateway", "app-gateway"]),
    ("front-door-and-cdn-profiles", &["front-door", "cdn"]),
    ("cache-redis", &["redis"]),
    ("monitor", &["monitoring"]),
    ("container-registries", &["acr", "container-registry"]),
    ("container-instances", &["aci"]),
    ("entra-id", &["aad", "active-directory"]),
];

fn azure(segments: &[&str], stem: &str) -> Option<Candidate> {
    let (_, service) = stem.split_once("-icon-service-")?;
    let full = slug(service);
    let id = strip_prefix_word(&full, &["azure-", "microsoft-"]).to_owned();
    let group = segments.len().checked_sub(2).map(|index| display_name(segments[index]));
    let mut aliases = vec![full];
    for (known, short_names) in AZURE_ALIASES {
        if id == *known {
            aliases.extend(short_names.iter().map(|alias| (*alias).to_owned()));
        }
    }
    // Several categories ship the same icon: the first category (alphabetical path order) wins.
    Some(Candidate { id, name: display_name(service), aliases, group, rank: 0, file: 0 })
}

// ---------------------------------------------------------------------------------------------------------------
// Kubernetes: kubernetes/community icons/svg/<kind>/<labeled|unlabeled>/<abbreviation>.svg

/// Abbreviations of the Kubernetes icon set → full name (the id) and extra aliases.
const KUBERNETES_NAMES: &[(&str, &str, &[&str])] = &[
    ("pod", "pod", &["pods"]),
    ("deploy", "deployment", &["deploy"]),
    ("ds", "daemonset", &["ds", "daemon-set"]),
    ("sts", "statefulset", &["sts", "stateful-set"]),
    ("rs", "replicaset", &["rs", "replica-set"]),
    ("svc", "service", &["svc"]),
    ("ing", "ingress", &["ing"]),
    ("ns", "namespace", &["ns"]),
    ("cm", "configmap", &["cm", "config-map"]),
    ("secret", "secret", &[]),
    ("pv", "persistent-volume", &["pv"]),
    ("pvc", "persistent-volume-claim", &["pvc"]),
    ("vol", "volume", &["vol"]),
    ("sc", "storage-class", &["sc"]),
    ("hpa", "horizontal-pod-autoscaler", &["hpa", "autoscaler"]),
    ("job", "job", &[]),
    ("cronjob", "cronjob", &["cron-job"]),
    ("ep", "endpoints", &["ep", "endpoint"]),
    ("netpol", "network-policy", &["netpol"]),
    ("sa", "service-account", &["sa"]),
    ("role", "role", &[]),
    ("c-role", "cluster-role", &["c-role", "clusterrole"]),
    ("rb", "role-binding", &["rb"]),
    ("crb", "cluster-role-binding", &["crb"]),
    ("crd", "custom-resource-definition", &["crd"]),
    ("quota", "resource-quota", &["quota"]),
    ("limits", "limit-range", &["limits"]),
    ("psp", "pod-security-policy", &["psp"]),
    ("user", "user", &[]),
    ("group", "group", &[]),
    ("api", "api-server", &["api", "kube-apiserver", "apiserver"]),
    ("c-c-m", "cloud-controller-manager", &["c-c-m", "ccm"]),
    ("c-m", "controller-manager", &["c-m", "kube-controller-manager"]),
    ("k-proxy", "kube-proxy", &["k-proxy", "proxy"]),
    ("kubelet", "kubelet", &[]),
    ("sched", "scheduler", &["sched", "kube-scheduler"]),
    ("control-plane", "control-plane", &["master"]),
    ("etcd", "etcd", &[]),
    ("node", "node", &["worker"]),
];

fn kubernetes(segments: &[&str], stem: &str) -> Option<Candidate> {
    let svg_root = segments.iter().position(|segment| *segment == "svg")?;
    let kind = segments.get(svg_root + 1)?;
    let variant = segments.get(svg_root + 2)?;
    // Unlabeled artwork reads better small; labeled is the fallback (control-plane components only ship it).
    let rank = match *variant {
        "unlabeled" => 0,
        "labeled" => 1,
        _ => return None,
    };
    let (id, aliases) = KUBERNETES_NAMES.iter().find(|(abbreviation, _, _)| *abbreviation == stem).map_or_else(
        || (slug(stem), Vec::new()),
        |(_, id, aliases)| ((*id).to_owned(), aliases.iter().map(|a| (*a).to_owned()).collect()),
    );
    let name = format!("Kubernetes {}", display_name(&id));
    Some(Candidate { id, name, aliases, group: Some(display_name(kind)), rank, file: 0 })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(paths: &[&str]) -> Vec<SourceFile> {
        paths.iter().map(|path| SourceFile { path: (*path).to_owned(), svg: String::new() }).collect()
    }

    fn ids(provider: Provider, paths: &[&str]) -> Vec<(String, Vec<String>, usize)> {
        normalize(provider, &files(paths)).into_iter().map(|icon| (icon.id, icon.aliases, icon.file)).collect()
    }

    #[test]
    fn slugs_are_kebab_case() {
        assert_eq!(slug("AWS-Lambda"), "aws-lambda");
        assert_eq!(
            slug("Machine-Learning-Studio-(Classic)-Web-Services"),
            "machine-learning-studio-classic-web-services"
        );
        assert_eq!(slug("AWS-Elemental-Appliances-&-Software"), "aws-elemental-appliances-and-software");
        assert_eq!(slug("  Cloud Storage "), "cloud-storage");
        assert_eq!(slug("__"), "");
    }

    #[test]
    fn aws_prefers_64_px_service_icons_and_short_names() {
        let icons = ids(
            Provider::Aws,
            &[
                "Architecture-Service-Icons_07312026/Arch_Compute/32/Arch_AWS-Lambda_32.svg",
                "Architecture-Service-Icons_07312026/Arch_Compute/64/Arch_AWS-Lambda_64.svg",
                "__MACOSX/Architecture-Service-Icons_07312026/Arch_Compute/64/._Arch_AWS-Lambda_64.svg",
                "Architecture-Service-Icons_07312026/Arch_Storage/64/Arch_Amazon-Simple-Storage-Service_64.svg",
                "Architecture-Service-Icons_07312026/Arch_Compute/64/Arch_AWS-Lambda_64.png",
                "Resource-Icons_07312026/Res_Storage/Res_Amazon-Simple-Storage-Service_Bucket_48.svg",
                "Architecture-Service-Icons_07312026/Arch_General-Icons/64/Arch_AWS-Marketplace_Dark_64.svg",
                "Architecture-Service-Icons_07312026/Arch_General-Icons/64/Arch_AWS-Marketplace_Light_64.svg",
                "Architecture-Group-Icons_07312026/Region_32.svg",
            ],
        );
        let lambda = ids_of(&icons, "lambda");
        assert_eq!(lambda.2, 1, "the 64 px file wins");
        assert!(lambda.1.contains(&"aws-lambda".to_owned()) && lambda.1.contains(&"function".to_owned()));
        assert!(ids_of(&icons, "simple-storage-service").1.contains(&"s3".to_owned()));
        let bucket = ids_of(&icons, "simple-storage-service-bucket");
        assert!(bucket.1.contains(&"s3-bucket".to_owned()), "{bucket:?}");
        assert!(!bucket.1.contains(&"bucket-bucket".to_owned()), "{bucket:?}");
        assert_eq!(ids_of(&icons, "marketplace").2, 7);
        assert_eq!(icons.len(), 4);
    }

    fn ids_of<'a>(icons: &'a [(String, Vec<String>, usize)], id: &str) -> &'a (String, Vec<String>, usize) {
        icons.iter().find(|icon| icon.0 == id).unwrap_or_else(|| panic!("{id} missing in {icons:?}"))
    }

    #[test]
    fn gcp_merges_core_category_and_legacy_archives() {
        let icons = ids(
            Provider::Gcp,
            &[
                "Unique Icons/BigQuery/SVG/BigQuery-512-color.svg",
                "Unique Icons/Cloud Run/SVG/CloudRun-512-color-rgb.svg",
                "Unique Icons/Cloud Run/SVG/CloudRun-512-color.svg",
                "Category Icons/Databases/SVG/Databases-512-color.svg",
                "bigquery/bigquery.svg",
                "pubsub/pubsub.svg",
                "cloud_storage/cloud_storage.svg",
                "pubsub/other.svg",
            ],
        );
        assert_eq!(ids_of(&icons, "bigquery").2, 0, "core artwork beats legacy");
        assert_eq!(ids_of(&icons, "cloud-run").2, 1, "RGB variant first");
        assert!(ids_of(&icons, "cloud-run").1.contains(&"run".to_owned()));
        assert!(ids_of(&icons, "cloud-storage").1.contains(&"gcs".to_owned()));
        assert!(ids_of(&icons, "pubsub").1.contains(&"pub-sub".to_owned()));
        assert_eq!(ids_of(&icons, "category-databases").2, 3);
        assert_eq!(icons.len(), 5);
        assert_eq!(title_case("cloud_sql"), "Cloud SQL");
    }

    #[test]
    fn azure_strips_numbers_and_prefixes() {
        let icons = ids(
            Provider::Azure,
            &[
                "Azure_Public_Service_Icons/Icons/compute/10029-icon-service-Function-Apps.svg",
                "Azure_Public_Service_Icons/Icons/iot/10029-icon-service-Function-Apps.svg",
                "Azure_Public_Service_Icons/Icons/databases/10121-icon-service-Azure-Cosmos-DB.svg",
                "Azure_Public_Service_Icons/Azure_Icons_FAQ.pdf",
            ],
        );
        let functions = ids_of(&icons, "function-apps");
        assert_eq!(functions.2, 0);
        assert!(functions.1.contains(&"functions".to_owned()));
        assert!(ids_of(&icons, "cosmos-db").1.contains(&"azure-cosmos-db".to_owned()));
        assert_eq!(icons.len(), 2);
    }

    #[test]
    fn kubernetes_prefers_unlabeled_and_expands_abbreviations() {
        let icons = ids(
            Provider::Kubernetes,
            &[
                "community-main/icons/svg/resources/labeled/pod.svg",
                "community-main/icons/svg/resources/unlabeled/pod.svg",
                "community-main/icons/svg/resources/unlabeled/deploy.svg",
                "community-main/icons/svg/control_plane_components/labeled/api.svg",
                "community-main/icons/png/resources/unlabeled/pod-128.png",
                "community-main/icons/svg/README.svg",
            ],
        );
        assert_eq!(ids_of(&icons, "pod").2, 1);
        assert!(ids_of(&icons, "deployment").1.contains(&"deploy".to_owned()));
        assert!(ids_of(&icons, "api-server").1.contains(&"kube-apiserver".to_owned()));
        assert_eq!(icons.len(), 3);
    }

    #[test]
    fn aliases_never_shadow_ids() {
        let icons = ids(
            Provider::Aws,
            &[
                "Arch_Compute/64/Arch_AWS-Lambda_64.svg",
                // A real icon whose id is the alias of another one keeps it.
                "Arch_Compute/64/Arch_AWS-Function_64.svg",
            ],
        );
        assert!(!ids_of(&icons, "lambda").1.contains(&"function".to_owned()));
    }
}
