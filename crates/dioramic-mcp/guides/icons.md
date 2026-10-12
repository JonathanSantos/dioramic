# Icons

Call `search_icons(query)` before using an icon you are not sure exists (`preview: true` returns a contact sheet image).
Aliases resolve to the canonical id, case-insensitive (`Postgres` → `database`).

| category | ids (some aliases) |
|---|---|
| compute | `server`, `rack`, `function` (lambda), `container` |
| network | `loadbalancer`, `switch`, `router`, `access-point`, `api-gateway`, `cdn`, `dns` |
| security | `firewall` |
| data | `database` (`postgres`, `mysql`, `db`), `cache` (`redis`), `storage` (`bucket`, `s3`, `nas`) |
| messaging | `queue` (`kafka`, `sqs`, `rabbitmq`) |
| cloud | `cloud` (`internet`) |
| clients | `users`, `person`, `workstation`, `laptop`, `mobile` |
| devices | `voip-phone`, `printer`, `atm` |
| k8s | `k8s-cluster`, `pod` (`k8s/pod`) |
| observability | `monitoring` (`prometheus`, `grafana`) |
| generic | `block` |

- `icon` omitted → inferred from the node id (`db` → database, `api1` → server), else `block`.
- Nothing fits? Use `icon: block` with a `color` and a clear `label`.
- An unknown explicit icon is an `E_UNKNOWN_ICON` error with a "did you mean" hint.
- The full catalog is the resource `dioramic://icons/catalog`.

## Provider icons (AWS, Google Cloud, Azure, Kubernetes)

Official provider icons are drawn flat on an isometric block: `aws/lambda`, `aws/s3`, `aws/dynamodb`, `gcp/bigquery`,
`gcp/cloud-run`, `azure/functions`, `azure/cosmos-db`, `k8s/pod`, `k8s/deployment`. They come from packs the user installs
once with `dioramic icons install aws|gcp|azure|k8s` (downloaded from the provider; not bundled).

- `search_icons` lists installed provider icons too and reports which packs are installed (`packs`); filter with `pack`.
- A provider icon whose pack is not installed still renders: a built-in stand-in is drawn and the warning
  `W_ICON_PACK_MISSING` tells the user the install command. Do not loop on it; mention it to the user.
- Project icons: every `icons/*.svg` next to the spec is `local/<file-name>` (`icons/stripe.svg` → `local/stripe`).
- Mix freely with built-in icons (`users`, `cloud`, `queue`).
