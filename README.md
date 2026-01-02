# RzGate HTTP Proxy

HTTP/JSON proxy for [Roomzin](https://m-javani.github.io/roomzin-doc/) — provides REST/JSON access to Roomzin's TCP-based inventory engine for legacy systems, teams that cannot use native SDKs, or quick testing environments.

RzGate is a high-performance HTTP proxy providing JSON access to the underlying Roomzin TCP-based backend. It is designed for maximum simplicity and speed — a single endpoint with command-based dispatching.

---

## Features

- Single unified endpoint with command-based routing
- Bearer token authentication with role-based access control
- Full CRUD support for all Roomzin operations
- Prometheus metrics endpoint for monitoring
- CORS support for cross-origin requests
- Hot-reloadable token configuration

---

## Requirements

- Roomzin cluster (recommended) **or** a single Roomzin instance in standalone mode
- For cluster mode: either a static `discovery.yml` or HTTP discovery endpoint
- For standalone mode: the address of the single Roomzin server
- Valid TLS certificates (production) or self-signed (testing)
- Roomzin tokens configured in `auth.yml`

---

## Installation

```bash
# Download the latest release
wget https://github.com/m-javani/rzgate/releases/latest/download/rzgate

# Make it executable
chmod +x rzgate
```

---

## CLI Options

| Flag | Description | Default |
|------|-------------|---------|
| `-c, --config` | Path to `rzgate.yml` | `./rzgate.yml` → `/etc/rzgate/rzgate.yml` |
| `-a, --auth-enabled` | Enable authentication/access control | `false` |
| `--http` | Enable HTTP protocol | `false` |
| `--https` | Enable HTTPS protocol | `false` |
| `-t, --tokens-path` | Path to `auth.yml` | `./auth.yml` → `/etc/rzgate/auth.yml` |
| `--tls-cert` | PEM certificate file | From `rzgate.yml` |
| `--tls-key` | PEM private key file | From `rzgate.yml` |
| `--discovery-kind` | Discovery mode: static or http | From config |
| `--discovery-yml-path` | Path to static discovery YAML file (for static mode) | From config |
| `--discovery-addr` | Discovery service URL (for http mode) | From config |
| `--no-cluster` | Run in standalone mode (single Roomzin node) | `false` |
| `--roomzin-standalone-host` | Address of Roomzin server (required with `--no-cluster`) | None |

### Run RZGate

```bash
# HTTP only (no auth)
./rzgate --http

# HTTPS with auth
./rzgate --https --auth-enabled --tokens-path ./auth.yml

# With custom config
./rzgate --config ./custom/rzgate.yml --http

# Standalone mode (single Roomzin node)
./rzgate --no-cluster --roomzin-standalone-host 127.0.0.1 --http
```

---

## Configuration (`rzgate.yml`)

A single YAML file controls every runtime setting. Keys are optional unless marked required; defaults are shown.

| Key | Purpose | Default |
|-----|---------|---------|
| `no_cluster` | Showing Roomzin running mode (single or clustered) | `false` |
| `roomzin_standalone_host` | Address of single mode Roomzin server (required with `--no-cluster`) | None |
| `roomzin_seed_ids` | Comma-separated list of initial Roomzin node ids | Required for clustered mode|
| `discovery_kind` | Discovery mode: static or http | static |
| `discovery_yml_path` | Path to YAML file with node ID → address mappings (required for static) | `./discovery.yml` |
| `discovery_addr` | HTTP endpoint that returns node ID → address mappings (required for http) | None |
| `discovery_refresh_interval_sec` | How often to fetch discovery data in HTTP mode | 2 |
| `roomzin_api_port` | Roomzin cluster HTTP health/portfolio port | `8080` |
| `roomzin_tcp_port` | Roomzin native TCP protocol port | `7777` |
| `conn_per_roomzin_node` | Number of TCP connections per Roomzin node | `1` |
| `listening_addr` | Address RZGate listens on | `0.0.0.0` |
| `http_port` | Port for HTTP | `8777` |
| `https_port` | Port for HTTPS | `3443` |
| `http_enabled` | Enable HTTP access | `false` |
| `https_enabled` | Enable HTTPS access | `false` |
| `auth_enabled` | Enable authentication/access control | `false` |
| `tokens_path` | Path to bearer-token file (auto-reloaded) | `./auth.yml` |
| `tls_cert_path` | PEM certificate for HTTPS | `./cert.pem` |
| `tls_key_path` | PEM private key | `./key.pem` |
| `timeout_sec` | Request timeout for Roomzin operations | 2 |
| `http_timeout_sec` | HTTP request timeout | 2 |
| `keep_alive_sec` | TCP keepalive interval | 30 |
| `node_probe_interval_sec` | Node health check interval | 2 |
| `max_active_conns` | Maximum concurrent connections | `10000` |
| `worker_threads` | Tokio worker threads (`0` = auto) | `num_cpus * 3` |

## Standalone Mode (No Cluster)

RzGate supports running against a **single Roomzin instance** (useful for development, testing, or small deployments).

To enable standalone mode, set in `rzgate.yml`:

```yaml
no_cluster: true
roomzin_standalone_host: "127.0.0.1"
```

Or via CLI:

```bash
./rzgate --no-cluster --roomzin-standalone-host 127.0.0.1 --http
```

## Discovery Configuration

RzGate needs to know how to reach each Roomzin node in the cluster. The cluster nodes communicate with each other using internal node IDs and discovery, but RzGate as an external proxy needs actual network addresses (IP:port or hostname:port) to connect.

Discovery provides the mapping from node IDs to their external addresses. Two modes are supported:

### Static Discovery

RzGate loads the mapping once from a YAML file and never updates it. Use this when your cluster nodes have stable, predictable addresses.

**`discovery.yml`:**
```yaml
nodes:
  - node_id: node-1
    addr: 10.0.1.11
    port: 7777
  - node_id: node-2
    addr: 10.0.1.12
    port: 7777
  - node_id: node-3
    addr: 10.0.1.13
    port: 7777
```

Then in `rzgate.yml`:
```yaml
discovery_kind: "static"
discovery_yml_path: "./discovery.yml"
```

### HTTP Discovery

RzGate periodically fetches the mapping from an HTTP endpoint. Use this when cluster nodes are dynamic (e.g., Kubernetes pods with changing IPs).

**HTTP endpoint must return:**
```json
{
  "nodes": [
    {"node_id": "node-1", "addr": "10.0.1.11"},
    {"node_id": "node-2", "addr": "10.0.1.12"}
  ]
}
```

Then in `rzgate.yml`:

```yaml
discovery_kind: "http"
discovery_addr: "http://discovery-service:8080/nodes"
discovery_refresh_interval_sec: 2
```

### Important Notes

- `roomzin_seed_ids` in the config should be node IDs, not addresses (e.g., `"node-1,node-2,node-3"`)
- The discovery mapping is independent of the cluster's internal peer discovery
- In HTTP mode, RzGate will:
  - Fetch the mapping every `discovery_refresh_interval_sec` seconds
  - Continue using the last known good mapping if the endpoint is unreachable
  - Log errors but never crash - it will keep retrying
- In Static mode, RzGate will fail to start if:
  - The discovery YAML file doesn't exist
  - The nodes list is empty
- The `addr` field can be IP:port or hostname:port

### Example `rzgate.yml`

```yaml

# === Standalone mode  ===
# no_cluster: true
# roomzin_standalone_host: "127.0.0.1"

# === Clustered mode ===
roomzin_seed_ids: "roomzin-0,roomzin-1,roomzin-2"
# Discovery - choose one mode
discovery_kind: "static"  # or "http"
discovery_yml_path: "./discovery.yml"  # required for static
# discovery_addr: "http://discovery-service:8080/nodes"  # required for http
# discovery_refresh_interval_sec: 2  # optional, for http mode

# Optional with defaults
roomzin_api_port: 8080
roomzin_tcp_port: 7777
conn_per_roomzin_node: 1
listening_addr: "0.0.0.0"
http_port: 8777
https_port: 3443
http_enabled: true
https_enabled: false
auth_enabled: false
tokens_path: "./auth.yml"
tls_cert_path: "./certs/cert.pem"
tls_key_path: "./certs/key.pem"
timeout: 2s
http_timeout: 2s
keep_alive: 30s
node_probe_interval: 2s
max_active_conns: 10000
worker_threads: 0
```

---

## Auth File (`auth.yml`)

Bearer tokens are loaded once at start-up and watched for changes—no restart required.

```yaml
# Token RZGate itself uses to talk to the Roomzin cluster
roomzin_token: "abc123"

# Client tokens that can read AND write
full_access_tokens:
  - "rzgate123"
  - "rzgate456"

# Client tokens that can only query/search
read_only_tokens:
  - "partner789"
  - "externalABC"
```

- `roomzin_token` must match the token configured on your Roomzin nodes.
- Any string is valid; rotate by editing the file—RZGate reloads within seconds.
- Returning `401 Unauthorized`? The supplied bearer token is missing or not listed above.

---

## Authentication

All requests (except CORS preflight `OPTIONS`) require authentication via Bearer token:

```http
Authorization: Bearer <your-api-token>
Content-Type: application/json
```

Tokens are managed in `auth.yml` on the server:
- **Full-access tokens** — can execute all commands (including mutations/deletions)
- **Read-only tokens** — can only execute query/search commands (safe for external partners)

Invalid or missing token → `401 Unauthorized`

Tokens can be added/removed by editing `auth.yml` — changes are hot-reloaded within seconds (no restart needed).

---

## API Reference

**Base URL:** `https://your-rzgate-server.com/api`  
(All requests are **HTTPS** only)

### Single Endpoint

**`POST /api`**

### Request Format
```json
{
  "command": "SEARCHAVAIL",
  "body": {
    // command-specific fields go here
  }
}
```

### Success Response
```json
{
  "status": "success",
  // command-specific result fields
}
```
HTTP status: `200 OK`

### Error Response
```json
{
  "status": "error",
  "message": "human-readable error description"
}
```
HTTP status: `400` (client error), `401` (auth), `403` (forbidden command with read-only token), or `5xx` (server)

---

## Supported Commands

### 1. SETPROP – Create / Update a Property
```json
// REQUEST body
{
  "segment":       "DXB",
  "area":          "Downtown",
  "property_id":   "PROP-123",
  "property_type": "HOTEL",
  "category":      "5_STAR",
  "stars":         5,
  "latitude":      25.2048,
  "longitude":     55.2708,
  "amenities":     ["wifi","pool","gym"]
}

// RESPONSE body (HTTP-200)
{ "status": "success" }
```

---

### 2. SEARCHPROP – List Property IDs that Match Filters
```json
// REQUEST body
{
  "segment":   "DXB",
  "area":      "Downtown",        // optional
  "type":      "HOTEL",           // optional
  "stars":     5,                 // optional
  "category":  "5_STAR",          // optional
  "amenities": ["wifi"],          // optional
  "longitude": 55.27,             // optional
  "latitude":  25.20,             // optional
  "limit":     100                // optional
}

// RESPONSE body
{
  "status": "success",
  "properties": ["PROP-123","PROP-456"]
}
```

---

### 3. SEARCHAVAIL – Search Availability + Pricing for Date List
```json
// REQUEST body
{
  "segment":      "DXB",
  "room_type":    "DBL",
  "area":         "Downtown",      // optional
  "property_id":  "PROP-123",      // optional
  "type":         "HOTEL",         // optional
  "stars":        5,               // optional
  "category":     "5_STAR",        // optional
  "amenities":    ["wifi"],        // optional
  "longitude":    55.27,           // optional
  "latitude":     25.20,           // optional
  "date":         ["2024-07-01","2024-07-02"],
  "availability": 1,               // optional filter
  "final_price":  25000,           // optional filter
  "rate_features":["BAR"],         // optional filter
  "limit":        50               // optional
}

// RESPONSE body
{
  "status": "success",
  "properties": [
    {
      "property_id": "PROP-123",
      "days": [
        {
          "date": "2024-07-01",
          "availability": 4,
          "final_price": 24000,
          "rate_feature": ["BAR"]
        }
      ]
    }
  ]
}
```

---

### 4. SETROOMPKG – Full Replace of Room-Day Data
```json
// REQUEST body
{
  "property_id":   "PROP-123",
  "room_type":     "DBL",
  "date":          "2024-07-01",
  "availability":  5,          // optional
  "final_price":   22000,      // optional
  "rate_features": ["BAR"]     // optional
}

// RESPONSE body
{ "status": "success" }
```

---

### 5. SETROOMAVL – Set Availability Only (Returns New Value)
```json
// REQUEST body
{
  "property_id": "PROP-123",
  "room_type":   "DBL",
  "date":        "2024-07-01",
  "amount":      7
}

// RESPONSE body
{
  "status": "success",
  "availability": 7
}
```

---

### 6. INCROOMAVL – Increment Availability (Returns New Value)
```json
// REQUEST body (same as SETROOMAVL)
{
  "property_id": "PROP-123",
  "room_type":   "DBL",
  "date":        "2024-07-01",
  "amount":      1
}

// RESPONSE body
{
  "status": "success",
  "availability": 8
}
```

---

### 7. DECROOMAVL – Decrement Availability (Returns New Value)
```json
// REQUEST body (same as SETROOMAVL)
{
  "property_id": "PROP-123",
  "room_type":   "DBL",
  "date":        "2024-07-01",
  "amount":      1
}

// RESPONSE body
{
  "status": "success",
  "availability": 6
}
```

---

### 8. PROPEXIST – Check if a Property Exists
```json
// REQUEST body
{ "property_id": "PROP-123" }

// RESPONSE body
{
  "status": "success",
  "exists": true
}
```

---

### 9. PROPROOMEXIST – Check if Property Has a Room Type
```json
// REQUEST body
{
  "property_id": "PROP-123",
  "room_type":   "DBL"
}

// RESPONSE body
{
  "status": "success",
  "exists": true
}
```

---

### 10. PROPROOMLIST – List All Room Types for a Property
```json
// REQUEST body
{ "property_id": "PROP-123" }

// RESPONSE body
{
  "status": "success",
  "room_types": ["DBL","KNG","SUI"]
}
```

---

### 11. PROPROOMDATELIST – List All Dates that Have Data for a Room Type
```json
// REQUEST body
{
  "property_id": "PROP-123",
  "room_type":   "DBL"
}

// RESPONSE body
{
  "status": "success",
  "dates": ["2024-07-01","2024-07-02"]
}
```

---

### 12. GETPROPROOMDAY – Fetch Single Room-Day Record
```json
// REQUEST body
{
  "property_id": "PROP-123",
  "room_type":   "DBL",
  "date":        "2024-07-01"
}

// RESPONSE body
{
  "status": "success",
  "property_id":  "PROP-123",
  "date":         "2024-07-01",
  "availability": 4,
  "final_price":  24000,
  "rate_feature": ["BAR"]
}
```

---

### 13. DELPROP – Delete Entire Property
```json
// REQUEST body
{ "property_id": "PROP-123" }

// RESPONSE body
{ "status": "success" }
```

---

### 14. DELSEGMENT – Delete Whole Segment
```json
// REQUEST body
{ "segment": "DXB" }

// RESPONSE body
{ "status": "success" }
```

---

### 15. DELPROPDAY – Delete All Room Data for a Property on One Date
```json
// REQUEST body
{
  "property_id": "PROP-123",
  "date":        "2024-07-01"
}

// RESPONSE body
{ "status": "success" }
```

---

### 16. DELPROPROOM – Delete a Room Type from a Property
```json
// REQUEST body
{
  "property_id": "PROP-123",
  "room_type":   "DBL"
}

// RESPONSE body
{ "status": "success" }
```

---

### 17. DELROOMDAY – Delete a Single Room-Day Record
```json
// REQUEST body
{
  "property_id": "PROP-123",
  "room_type":   "DBL",
  "date":        "2024-07-01"
}

// RESPONSE body
{ "status": "success" }
```

---

### 18. GETSEGMENTS – List All Segments with Property Count
```json
// REQUEST body
{}  // empty object

// RESPONSE body
{
  "status": "success",
  "segments": [
    {
      "segment":   "DXB",
      "prop_count": 1523
    }
  ]
}
```

---

## Error Shape (HTTP-200 Body, Any Command)
```json
{
  "status": "error",
  "message": "property not found"
}
```

---

## OpenAPI Specification

RzGate provides a complete [OpenAPI 3.1.1](https://swagger.io/specification/) specification that describes the entire API surface, including all commands, request/response formats, and authentication requirements.

The specification can be used to:
- Generate client libraries in any language
- Validate API requests and responses
- Generate interactive API documentation
- Integrate with API gateways and testing tools

### Download the Specification

The latest OpenAPI specification is available at:
- **Raw YAML**: [`openapi.yaml`](openapi.yaml)

### Generating Client Code

You can use the specification to generate a client for your preferred language. Here are examples using the most popular tools:

OpenAPI Generator supports [over 50 languages and frameworks](https://openapi-generator.tech/docs/generators/). To generate a client for a specific language:

---

## Metrics

RzGate exposes a Prometheus endpoint for monitoring its performance and the health of the backend Roomzin cluster.

**Endpoint:** `GET /metrics`

  **Authentication:** The endpoint is protected by the same bearer-token authentication as every other route. Any valid token (read-only or full-access) will work.

### Exported Metrics

| Metric Name | Type | Description |
|-------------|------|-------------|
| `api_commands_total` | Counter | Total JSON commands processed by RzGate |
| `api_bytes_received_total` | Counter | HTTP request body bytes received |
| `api_bytes_sent_total` | Counter | HTTP response body bytes sent |
| `api_client_errors_total` | Counter | 4xx / 5xx responses sent to clients |
| `api_client_login_fail_total` | Counter | Rejected bearer tokens (authentication failures) |
| `backend_followers_total` | Gauge | Number of live Roomzin follower nodes RzGate is connected to |
| `backend_leader_change_total` | Counter | Leader elections detected since startup |
| `backend_disconnect_total` | Counter | TCP disconnections from any Roomzin node |

---

### Operational Notes

- **No IP filtering:** RzGate does not restrict access to `/metrics` by IP or token type. If you want to hide it from the internet, block the path at your load balancer, firewall, or Prometheus sidecar.
- **High availability:** Running multiple RzGate instances behind a load balancer? Each instance exposes its own metrics. A Prometheus service discovery setup (e.g., `file_sd_configs`) is recommended for monitoring all instances.
- **Token requirements:** The bearer token used for scraping must be valid. We recommend using a dedicated read-only token for monitoring purposes.


---

## Important Notes for Developers & Integrators
- All strings are **case-sensitive**.
- Dates must be exactly `YYYY-MM-DD`.
- Use read-only tokens for any external/partner integrations — they cannot perform mutations.
- `SEARCHAVAIL` is heavily optimized — keep queries focused (use filters, reasonable date ranges, `limit`).
- Optional fields can be omitted or set to `null`.
- All destructive commands (`DEL*`) are **irreversible** and require full-access token.
- No session/state — each request is independent and authenticated via Bearer token.
- CORS is enabled for all origins (GET/POST/OPTIONS).

**for standalone mode:**
- Discovery settings (`discovery_kind`, `discovery_yml_path`, `discovery_addr`) are ignored.
- All requests (reads and writes) go to the same single node.
- You cannot combine `--no-cluster` with discovery-related CLI flags.

**for clustered mode:**
- `roomzin_seed_ids` must match the `node_id` values in your discovery mapping
- Discovery provides the address mapping; the cluster provides node roles (leader/follower)


---

## Contributing

Contributions are welcome!

Please open an issue before proposing large changes. All contributions are subject to the BUSL-1.1 License terms.

---

## License

This project is licensed under the [BUSL-1.1 License](LICENSE).

**Note:** RzGate is designed to communicate with Roomzin Server, which requires a valid Roomzin license.

---

## Support

- **Documentation**: [roomzin-doc/rzgate](https://m-javani.github.io/roomzin-doc/rzgate.html)
- **Community Q&A**: [GitHub Discussions](https://github.com/m-javani/roomzin-doc/discussions)
- **Issues**: [GitHub Issues](https://github.com/m-javani/rzgate/issues)
- **Security**: [mehdy.javany@gmail.com](mailto:mehdy.javany@gmail.com)

---

## Related Repositories

- [Roomzin Quickstart](https://github.com/m-javani/roomzin-quickstart) — Local Docker cluster
- [Roomzin Bench](https://github.com/m-javani/roomzin-bench) — Benchmarking tool
- [Roomzin SDKs](https://github.com/m-javani?tab=repositories&q=roomzin) — Native SDKs for all languages