//! The static icon catalog (procedural icons of v1). T-102 may add aliases/keywords and entries.

use super::{IconCategory, IconEntry, IconKind, ProceduralIcon};

macro_rules! entry {
    ($id:literal, $name:literal, $icon:ident, $category:ident, [$($alias:literal),* $(,)?]) => {
        IconEntry {
            id: $id,
            name: $name,
            aliases: &[$($alias),*],
            category: IconCategory::$category,
            kind: IconKind::Procedural(ProceduralIcon::$icon),
        }
    };
}

/// Every icon, ordered by category then id (the order of `icons list` and the contact sheet).
pub(super) static CATALOG: &[IconEntry] = &[
    // compute
    entry!(
        "server",
        "Server",
        Server,
        Compute,
        ["host", "vm", "instance", "api", "app", "backend", "web", "ec2", "compute"]
    ),
    entry!("rack", "Rack", Rack, Compute, ["cabinet", "server-rack"]),
    entry!(
        "function",
        "Function",
        Function,
        Compute,
        ["lambda", "faas", "serverless", "cloud-function", "worker", "job", "cron"]
    ),
    entry!("container", "Container", Container, Compute, ["docker", "ecs", "fargate", "task", "microservice"]),
    // network
    entry!(
        "loadbalancer",
        "Load Balancer",
        LoadBalancer,
        Network,
        ["lb", "load-balancer", "alb", "elb", "nlb", "haproxy", "nginx", "ingress"]
    ),
    entry!("switch", "Switch", Switch, Network, ["network-switch", "l2-switch", "l3-switch"]),
    entry!("router", "Router", Router, Network, ["edge-router", "gateway-router"]),
    entry!("access-point", "Access Point", AccessPoint, Network, ["ap", "wifi", "wireless", "wlan"]),
    entry!("api-gateway", "API Gateway", ApiGateway, Network, ["apigw", "gateway", "api-management", "kong"]),
    entry!("cdn", "CDN", Cdn, Network, ["cloudfront", "edge-cache", "akamai", "fastly", "cloudflare"]),
    entry!("dns", "DNS", Dns, Network, ["route53", "nameserver", "domain"]),
    // security
    entry!("firewall", "Firewall", Firewall, Security, ["fw", "waf", "security-group"]),
    // data
    entry!(
        "database",
        "Database",
        Database,
        Data,
        [
            "db",
            "postgres",
            "postgresql",
            "pg",
            "mysql",
            "mariadb",
            "sql",
            "rds",
            "aurora",
            "oracle",
            "mongodb",
            "mongo",
            "dynamodb",
        ]
    ),
    entry!("cache", "Cache", Cache, Data, ["redis", "memcached", "valkey", "elasticache"]),
    entry!("storage", "Storage", Storage, Data, ["bucket", "s3", "blob", "object-storage", "gcs", "data-lake", "lake"]),
    entry!("nas", "NAS", Nas, Data, ["san", "disk-array", "disk", "volume", "backup", "file-server"]),
    // messaging
    entry!(
        "queue",
        "Queue",
        Queue,
        Messaging,
        ["mq", "kafka", "rabbitmq", "sqs", "pubsub", "broker", "stream", "kinesis", "nats", "topic", "event-bus",]
    ),
    // cloud
    entry!("cloud", "Cloud", Cloud, Cloud, ["internet", "external", "wan", "saas", "third-party", "mpls"]),
    // clients
    entry!("users", "Users", Users, Clients, ["clients", "customers", "people", "audience"]),
    entry!("person", "Person", Person, Clients, ["user", "customer", "actor", "admin", "human"]),
    entry!("workstation", "Workstation", Workstation, Clients, ["desktop", "pc", "computer", "terminal", "screen"]),
    entry!("laptop", "Laptop", Laptop, Clients, ["notebook", "macbook"]),
    entry!("mobile", "Mobile", Mobile, Clients, ["phone", "smartphone", "ios", "android", "tablet", "mobile-app"]),
    // devices
    entry!("voip-phone", "VoIP Phone", VoipPhone, Devices, ["voip", "desk-phone", "telephone", "ip-phone"]),
    entry!("printer", "Printer", Printer, Devices, ["print", "mfp"]),
    entry!("atm", "ATM", Atm, Devices, ["cash-machine", "kiosk"]),
    // kubernetes
    entry!(
        "k8s-cluster",
        "Kubernetes Cluster",
        K8sCluster,
        Kubernetes,
        ["k8s", "kubernetes", "cluster", "eks", "gke", "aks"]
    ),
    entry!("pod", "Pod", Pod, Kubernetes, ["k8s/pod", "k8s-pod", "deployment", "replicaset"]),
    // observability
    entry!(
        "monitoring",
        "Monitoring",
        Monitoring,
        Observability,
        [
            "monitor",
            "observability",
            "metrics",
            "prometheus",
            "grafana",
            "dashboard",
            "alerting",
            "logs",
            "datadog",
            "tracing",
        ]
    ),
    // generic
    entry!("block", "Block", Block, Generic, ["box", "generic", "service", "component"]),
];
