//! The four provider packs: where their official archives live and what their terms say.
//!
//! Sources checked in 2026-10 (see `docs/02-decisoes.md#adr-007`): the project never redistributes these files;
//! `dioramic icons install` downloads them from the provider. Pages whose archive URL changes with every
//! release (AWS, Azure) are scraped for the current link, with the last known link as a fallback.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// A provider pack the CLI can install.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Provider {
    /// AWS Architecture Icons.
    #[serde(rename = "aws")]
    Aws,
    /// Google Cloud icons.
    #[serde(rename = "gcp")]
    Gcp,
    /// Azure architecture icons.
    #[serde(rename = "azure")]
    Azure,
    /// Kubernetes community icons.
    #[serde(rename = "k8s")]
    Kubernetes,
}

/// How to fetch a provider's archives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Download {
    /// Fixed archive URLs (all of them are merged into one pack).
    Archives(&'static [&'static str]),
    /// An official page that links the current archive; the first link matching `marker` and ending in
    /// `.zip` is used, else `fallback`.
    LinkedFrom {
        /// Page to read.
        page: &'static str,
        /// Substring identifying the archive link (`Icon-package_`).
        marker: &'static str,
        /// Last known archive URL.
        fallback: &'static str,
    },
}

impl Provider {
    /// Every provider, in `icons install` order.
    pub const ALL: [Provider; 4] = [Self::Aws, Self::Gcp, Self::Azure, Self::Kubernetes];

    /// Pack id (`aws`, `gcp`, `azure`, `k8s`), also the icon id prefix.
    pub fn id(self) -> &'static str {
        match self {
            Self::Aws => "aws",
            Self::Gcp => "gcp",
            Self::Azure => "azure",
            Self::Kubernetes => "k8s",
        }
    }

    /// Display name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Aws => "AWS Architecture Icons",
            Self::Gcp => "Google Cloud icons",
            Self::Azure => "Azure architecture icons",
            Self::Kubernetes => "Kubernetes icons (kubernetes/community)",
        }
    }

    /// A well-known icon id of the pack, for hints (`lambda` → `icon: aws/lambda`).
    pub fn example_icon(self) -> &'static str {
        match self {
            Self::Aws => "lambda",
            Self::Gcp => "bigquery",
            Self::Azure => "function-apps",
            Self::Kubernetes => "pod",
        }
    }

    /// Official page of the icon set.
    pub fn homepage(self) -> &'static str {
        match self {
            Self::Aws => "https://aws.amazon.com/architecture/icons/",
            Self::Gcp => "https://cloud.google.com/icons",
            Self::Azure => "https://learn.microsoft.com/azure/architecture/icons/",
            Self::Kubernetes => "https://github.com/kubernetes/community/tree/main/icons",
        }
    }

    /// Terms of use as stated by the provider (shown before downloading and stored in the manifest).
    pub fn license(self) -> &'static str {
        match self {
            Self::Aws => {
                "AWS permits customers and partners to use these icons to create architecture diagrams \
                 (https://aws.amazon.com/architecture/icons/). Not redistributed by Dioramic."
            }
            Self::Gcp => {
                "Google Cloud icons for architecture diagrams (https://cloud.google.com/icons). \
                 Not redistributed by Dioramic."
            }
            Self::Azure => {
                "Microsoft permits the use of these icons in architectural diagrams, training materials, or \
                 documentation; Microsoft reserves all other rights (https://learn.microsoft.com/azure/architecture/icons/). \
                 Not redistributed by Dioramic."
            }
            Self::Kubernetes => "Apache-2.0 (https://github.com/kubernetes/community/blob/main/LICENSE)",
        }
    }

    /// Where the official archives are.
    pub fn download(self) -> Download {
        match self {
            Self::Aws => Download::LinkedFrom {
                page: "https://aws.amazon.com/architecture/icons/",
                marker: "Icon-package_",
                fallback: "https://d1.awsstatic.com/onedam/marketing-channels/website/public/shared/architecture-icon-release/Icon-package_07312026.5846e92413caa21490223536cc97f1269e44fa92.zip",
            },
            Self::Gcp => Download::Archives(&[
                "https://services.google.com/fh/files/misc/google-cloud-legacy-icons.zip",
                "https://services.google.com/fh/files/misc/category-icons.zip",
                "https://services.google.com/fh/files/misc/core-products-icons.zip",
            ]),
            Self::Azure => Download::LinkedFrom {
                page: "https://learn.microsoft.com/en-us/azure/architecture/icons/",
                marker: "arch-center.azureedge.net/icons/",
                fallback: "https://arch-center.azureedge.net/icons/Azure_Public_Service_Icons_V24.zip",
            },
            Self::Kubernetes => {
                Download::Archives(&["https://github.com/kubernetes/community/archive/refs/heads/main.zip"])
            }
        }
    }

    /// Release named by archive file names (`Icon-package_07312026` → `2026-07-31`, `…_V24.zip` → `V24`).
    pub fn version_from(self, names: &[String]) -> Option<String> {
        names.iter().find_map(|name| match self {
            Self::Aws => aws_date(name),
            Self::Azure => azure_version(name),
            Self::Gcp | Self::Kubernetes => None,
        })
    }
}

/// `…_07312026…` (MMDDYYYY) → `2026-07-31`.
fn aws_date(name: &str) -> Option<String> {
    name.split(|character: char| !character.is_ascii_digit())
        .find(|digits| digits.len() == 8)
        .map(|digits| format!("{}-{}-{}", &digits[4..], &digits[..2], &digits[2..4]))
}

/// `Azure_Public_Service_Icons_V24.zip` → `V24`.
fn azure_version(name: &str) -> Option<String> {
    let start = name.rfind("_V")? + 1;
    let digits: String = name[start + 1..].chars().take_while(char::is_ascii_digit).collect();
    (!digits.is_empty()).then(|| format!("V{digits}"))
}

/// First absolute `https://…zip` link in `html` that contains `marker`.
pub fn find_archive_link(html: &str, marker: &str) -> Option<String> {
    let mut rest = html;
    while let Some(start) = rest.find("https://") {
        let candidate = &rest[start..];
        let end = candidate
            .find(|character: char| {
                character == '"'
                    || character == '\''
                    || character == '<'
                    || character.is_whitespace()
                    || character == ')'
            })
            .unwrap_or(candidate.len());
        let url = &candidate[..end];
        if url.contains(marker) && url.to_ascii_lowercase().ends_with(".zip") {
            return Some(url.replace("&amp;", "&"));
        }
        rest = &candidate[end.max(1)..];
    }
    None
}

impl fmt::Display for Provider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}

/// Error of [`Provider::from_str`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown icon pack '{0}': use aws, gcp, azure or k8s")]
pub struct UnknownProvider(pub String);

impl FromStr for Provider {
    type Err = UnknownProvider;

    /// `aws`, `gcp` (`google`), `azure`, `k8s` (`kubernetes`), case-insensitive.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        match text.trim().to_ascii_lowercase().as_str() {
            "aws" | "amazon" => Ok(Self::Aws),
            "gcp" | "google" | "google-cloud" => Ok(Self::Gcp),
            "azure" | "microsoft" => Ok(Self::Azure),
            "k8s" | "kubernetes" => Ok(Self::Kubernetes),
            _ => Err(UnknownProvider(text.to_owned())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ids_and_aliases() {
        assert_eq!("AWS".parse::<Provider>(), Ok(Provider::Aws));
        assert_eq!("kubernetes".parse::<Provider>(), Ok(Provider::Kubernetes));
        assert!("ibm".parse::<Provider>().is_err());
        assert!(Provider::ALL.iter().all(|provider| provider.id().parse::<Provider>() == Ok(*provider)));
    }

    #[test]
    fn finds_the_current_archive_on_the_page() {
        let html = r#"<a href="https://d1.awsstatic.com/x/Microsoft-PPTx-toolkits_07312026.1c.zip">PPT</a>
            <a href='https://d1.awsstatic.com/x/Icon-package_07312026.58.zip'>Icons</a>"#;
        assert_eq!(
            find_archive_link(html, "Icon-package_").as_deref(),
            Some("https://d1.awsstatic.com/x/Icon-package_07312026.58.zip")
        );
        let azure = "[Download SVG icons](https://arch-center.azureedge.net/icons/Azure_Public_Service_Icons_V25.zip)";
        assert_eq!(
            find_archive_link(azure, "arch-center.azureedge.net/icons/").as_deref(),
            Some("https://arch-center.azureedge.net/icons/Azure_Public_Service_Icons_V25.zip")
        );
        assert_eq!(find_archive_link("no links", "x"), None);
    }

    #[test]
    fn reads_versions_from_archive_names() {
        let names = |name: &str| vec![name.to_owned()];
        assert_eq!(
            Provider::Aws.version_from(&names("Icon-package_07312026.5846e9.zip")).as_deref(),
            Some("2026-07-31")
        );
        assert_eq!(Provider::Azure.version_from(&names("Azure_Public_Service_Icons_V24.zip")).as_deref(), Some("V24"));
        assert_eq!(Provider::Gcp.version_from(&names("core-products-icons.zip")), None);
    }
}
