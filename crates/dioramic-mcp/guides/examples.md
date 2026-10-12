# Examples

Complete specs are available as resources `dioramic://examples/{name}`:
`minimal`, `checkout`, `bank-branch`, `kubernetes-platform`, `event-driven`, `ml-pipeline`.

| name | shows |
|---|---|
| `minimal` | 4 nodes, no zones, no coordinates, no colors (start like this) |
| `checkout` | 4 zones in a line, theme styles (`@primary`, `@secondary`), `animation: { preset: flow }` |
| `bank-branch` | 17 nodes, named styles, nested zone (`parent`) without border, `..` edge |
| `kubernetes-platform` | namespaces as nested zones, `replicas`, `badge`, `midnight`, `density: compact` |
| `event-driven` | async `..>` edges, failure style, node `color`, `paper`, `rotation: auto` |
| `ml-pipeline` | long flow in 5 zones, feedback loop with a dashed edge |

## checkout

```yaml
diorama: 1
title: Checkout Service
subtitle: request path · rendered by dioramic
theme: blueprint

zones:
  - { id: internet, label: INTERNET, color: violet }
  - { id: edge,     label: EDGE,     color: blue }
  - { id: app,      label: APP TIER, color: orange }
  - { id: data,     label: DATA,     color: teal }

nodes:
  - { id: users, icon: cloud,        label: Users,         zone: internet }
  - { id: lb,    icon: loadbalancer, label: Load Balancer, zone: edge }
  - { id: api1,  icon: server,       label: "API #1",      zone: app }
  - { id: api2,  icon: server,       label: "API #2",      zone: app }
  - { id: db,    icon: postgres,     label: Postgres,      zone: data }
  - { id: cache, icon: redis,        label: Redis,         zone: data }

edges:
  - users -> lb: HTTPS @primary
  - lb -> api1 @primary
  - lb -> api2: round-robin @primary
  - api1 -> db: SQL @secondary
  - api2 -> db @secondary
  - api2 -> cache: GET / SET @secondary

animation: { preset: flow }
```

## event-driven

```yaml
diorama: 1
title: Order Events Pipeline
subtitle: async fan-out with a durable stream
theme: paper

view: { rotation: auto }

styles:
  sync:  { color: blue }
  async: { color: orange, dashed: true }
  fail:  { color: red, dashed: true }

zones:
  - { id: producers, label: Producers, color: blue }
  - { id: broker,    label: Event Bus, color: orange }
  - { id: consumers, label: Consumers, color: green }
  - { id: sinks,     label: Storage & Notify, color: teal }

nodes:
  - { id: mobile_app, icon: mobile,      label: Mobile App,     zone: producers }
  - { id: api,        icon: api-gateway, label: Orders API,     zone: producers }
  - { id: stream,     icon: queue,       label: orders.events,  zone: broker }
  - { id: dlq,        icon: queue,       label: Dead Letter Q,  zone: broker, color: red }
  - { id: billing,    icon: function,    label: Billing Worker, zone: consumers }
  - { id: shipping,   icon: container,   label: Shipping Svc,   zone: consumers, replicas: 2 }
  - { id: archiver,   icon: function,    label: Event Archiver, zone: consumers }
  - { id: ledger,     icon: database,    label: Ledger DB,      zone: sinks }
  - { id: lake,       icon: storage,     label: Event Archive,  zone: sinks }
  - { id: notify,     icon: mobile,      label: Push / Email,   zone: sinks }

edges:
  - mobile_app -> api: POST /orders @sync
  - api ..> stream: OrderPlaced @async
  - stream ..> billing: subscribe @async
  - stream ..> shipping: subscribe @async
  - stream ..> archiver: subscribe @async
  - billing -> ledger: INSERT @sync
  - archiver -> lake: Parquet batches @sync
  - shipping ..> notify: ShipmentCreated @async
  - billing ..> dlq: on failure @fail

animation: { preset: flow }
```
