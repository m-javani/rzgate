# RzGate HTTP Proxy

HTTP/JSON proxy for [Roomzin](https://m-javani.github.io/roomzin-doc/) — provides REST/JSON access to Roomzin's TCP-based inventory engine for legacy systems, teams that cannot use native SDKs, or quick testing environments.

RzGate is a high-performance HTTP proxy providing JSON access to the underlying Roomzin TCP-based backend. It is designed for maximum simplicity and speed — a single endpoint with command-based dispatching.

---

## Features

- Single unified endpoint with command-based routing
- Full CRUD support for all Roomzin operations
- Prometheus metrics endpoint for monitoring
- CORS support for cross-origin requests
- Simple configuration — just point it to your Roomzin server or router

---

## Requirements

- Roomzin standalone server **or** Roomzin cluster with router
- No authentication, TLS, or discovery configuration needed (handled by infrastructure)

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
| `--addr` | Server address (standalone host or router address) | From config |
| `--port` | TCP port | From config |
| `--mode` | `standalone` or `router` | From config |
| `--listening-addr` | Address RzGate listens on | From config |
| `--http-port` | HTTP port | From config |

### Run RzGate

```bash
# With config file
./rzgate --config ./rzgate.yml

# Override config values
./rzgate --addr router.example.com --port 7777 --mode router
```

---

## Configuration (`rzgate.yml`)

A single YAML file controls every runtime setting.

| Key | Purpose | Default |
|-----|---------|---------|
| `addr` | Server address (standalone host or router address) | Required |
| `port` | TCP port | Required |
| `mode` | `standalone` or `router` | `standalone` |
| `timeout_sec` | Request timeout | `2` |
| `keep_alive_sec` | TCP keepalive interval | `30` |
| `conn_per_node` | Number of TCP connections per node | `1` |
| `max_active_conns` | Maximum concurrent connections | `10000` |
| `worker_threads` | Tokio worker threads (`0` = auto) | `num_cpus * 3` |
| `listening_addr` | Address RzGate listens on | `0.0.0.0` |
| `http_port` | HTTP port | `8777` |

### Example `rzgate.yml`

```yaml
addr: "router.example.com" # | "127.0.0.1"
port: 7777
mode: "router" # | "standalone"
listening_addr: "0.0.0.0"
http_port: 8777
timeout_sec: 2
keep_alive_sec: 30
conn_per_node: 10
max_active_conns: 10000
worker_threads: 0
```

---

## API Reference

**Base URL:** `http://your-rzgate-server.com/api`

### Single Endpoint

**`POST /api`**

### Request Format

All requests require a `segment` field for routing (ignored in standalone mode).

```json
{
  "command": "SEARCHAVAIL",
  "segment": "downtown",
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
HTTP status: `400` (client error) or `5xx` (server)

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

// RESPONSE body
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

## Metrics

RzGate exposes a Prometheus endpoint for monitoring.

**Endpoint:** `GET /metrics`

### Exported Metrics

| Metric Name | Type | Description |
|-------------|------|-------------|
| `api_commands_total` | Counter | Total JSON commands processed |
| `api_bytes_received_total` | Counter | HTTP request body bytes received |
| `api_bytes_sent_total` | Counter | HTTP response body bytes sent |
| `api_client_errors_total` | Counter | 4xx / 5xx responses sent to clients |

---

## Important Notes for Developers & Integrators

- All strings are **case-sensitive**.
- Dates must be exactly `YYYY-MM-DD`.
- All commands require a `segment` field in the request root (ignored in standalone mode).
- `SEARCHAVAIL` is heavily optimized — keep queries focused (use filters, reasonable date ranges, `limit`).
- Optional fields can be omitted or set to `null`.
- All destructive commands (`DEL*`) are **irreversible**.
- No session/state — each request is independent.
- CORS is enabled for all origins (GET/POST/OPTIONS).

**For standalone mode:**
- The `segment` field is ignored but still required for API compatibility.
- All requests go to the single Roomzin node.

**For router mode:**
- The `segment` field is used by the router to route to the correct shard.
- RzGate connects to the router, which handles all routing logic.

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

---

## Related Repositories

- [Roomzin Quickstart](https://github.com/m-javani/roomzin-quickstart) — Local Docker cluster
- [Roomzin Bench](https://github.com/m-javani/roomzin-bench) — Benchmarking tool
- [Roomzin SDKs](https://github.com/m-javani?tab=repositories&q=roomzin) — Native SDKs for all languages