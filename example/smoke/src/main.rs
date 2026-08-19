use chrono::{Duration, Utc};
use reqwest;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::time::{Duration as StdDuration, Instant};
use tokio::time::sleep;

// ============================================================================
// CONFIGURATION
// ============================================================================

const RZProxy_URL: &str = "http://127.0.0.1:8777/api";
const TIMEOUT_SECS: u64 = 30;

// Test data parameters - matches smoke.go
const NUM_SEGMENTS: usize = 4;
const NUM_PROPS_PER_SEGMENT: usize = 10;
const NUM_ROOMS_PER_PROP: usize = 4;
const NUM_DAYS: usize = 10;
const SHARD_IDX: u32 = 1;

// ============================================================================
// TYPES
// ============================================================================

#[derive(Debug, Serialize)]
struct RzProxyRequest {
    command: String,
    segment: String,
    body: HashMap<String, Value>,
}

#[derive(Debug, Deserialize)]
struct RzProxyResponse {
    status: String,
    #[serde(default)]
    _data: Option<HashMap<String, Value>>,
}

// ============================================================================
// HTTP CLIENT WITH RETRY
// ============================================================================

struct RzProxyClient {
    url: String,
    client: reqwest::Client,
}

impl RzProxyClient {
    fn new(url: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(StdDuration::from_secs(TIMEOUT_SECS))
            .build()
            .expect("Failed to build HTTP client");

        RzProxyClient { url, client }
    }

    async fn do_request(
        &self,
        cmd: &str,
        segment: &str,
        body: HashMap<String, Value>,
    ) -> Result<RzProxyResponse, String> {
        let req = RzProxyRequest {
            command: cmd.to_string(),
            segment: segment.to_string(),
            body,
        };

        let json_data =
            serde_json::to_vec(&req).map_err(|e| format!("Failed to serialize request: {}", e))?;

        // Retry logic
        let max_retries = 3;
        let mut last_err = None;

        for attempt in 0..max_retries {
            if attempt > 0 {
                sleep(StdDuration::from_millis(100)).await;
                println!("        Retry {} for {}", attempt + 1, cmd);
            }

            let response = self
                .client
                .post(&self.url)
                .header("Content-Type", "application/json")
                .body(json_data.clone())
                .send()
                .await;

            match response {
                Ok(resp) => {
                    let result: RzProxyResponse = resp
                        .json()
                        .await
                        .map_err(|e| format!("Failed to parse response: {}", e))?;
                    return Ok(result);
                }
                Err(e) => {
                    last_err = Some(e.to_string());
                    continue;
                }
            }
        }

        Err(format!(
            "Failed after {} attempts: {}",
            max_retries,
            last_err.unwrap_or_else(|| "Unknown error".to_string())
        ))
    }
}

// ============================================================================
// TIMING HELPERS
// ============================================================================

struct StepTiming {
    name: String,
    duration: StdDuration,
    request_count: usize,
}

struct BenchResult {
    timings: Vec<StepTiming>,
}

impl BenchResult {
    fn new() -> Self {
        BenchResult {
            timings: Vec::new(),
        }
    }

    async fn time_step<F, Fut>(
        &mut self,
        name: &str,
        request_count: usize,
        mut f: F,
    ) -> Result<(), String>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<(), String>>,
    {
        let start = Instant::now();
        let result = f().await;
        let duration = start.elapsed();

        self.timings.push(StepTiming {
            name: name.to_string(),
            duration,
            request_count,
        });

        if let Err(ref err) = result {
            println!("  ❌ {} failed after {:?}", name, duration);
            println!("     Error: {}", err);
        } else {
            println!(
                "  ✅ {} completed in {:?} ({} requests)",
                name, duration, request_count
            );
        }
        result
    }

    fn print_summary(&self) {
        println!("\n{}", "=".repeat(61));
        println!("  RZProxy HTTP BENCHMARK SUMMARY");
        println!("{}", "=".repeat(61));

        let mut total_time = StdDuration::from_secs(0);
        let mut total_requests = 0;

        for t in &self.timings {
            total_time += t.duration;
            total_requests += t.request_count;
            println!(
                "  {:<25} {:>10?}  {:>4} requests",
                format!("{}:", t.name),
                t.duration,
                t.request_count
            );
        }

        println!("{}", "-".repeat(61));
        println!(
            "  {:<25} {:>10?}  {:>4} requests",
            "TOTAL:", total_time, total_requests
        );
        if total_requests > 0 {
            println!(
                "  {:<25} {:>10?}",
                "Avg per request:",
                total_time / total_requests as u32
            );
        }
        println!("{}", "=".repeat(61));
    }
}

// ============================================================================
// TEST RZProxy HEALTH
// ============================================================================

async fn test_rzproxy_health(url: &str) -> Result<(), String> {
    println!("Testing RzProxy connection...");

    let health_url = url.replace("/api", "/health");
    let client = reqwest::Client::builder()
        .timeout(StdDuration::from_secs(5))
        .build()
        .map_err(|e| format!("Failed to build client: {}", e))?;

    let response = client
        .get(&health_url)
        .send()
        .await
        .map_err(|e| format!("Health check failed: {}", e))?;

    if response.status() != reqwest::StatusCode::OK {
        return Err(format!("Health check returned {}", response.status()));
    }

    println!("✅ RzProxy is healthy");
    Ok(())
}

// ============================================================================
// MAIN
// ============================================================================

#[tokio::main]
async fn main() -> Result<(), String> {
    let client = RzProxyClient::new(RZProxy_URL.to_string());

    println!("=== RzProxy HTTP Benchmark ===");
    println!("URL: {}\n", RZProxy_URL);

    // Check if RzProxy is running
    if let Err(e) = test_rzproxy_health(RZProxy_URL).await {
        eprintln!("RzProxy not available: {}", e);
        return Err(e);
    }

    let mut bench_result = BenchResult::new();

    // -------------------------------------------------------------------------
    // STEP 1: SetProp
    // -------------------------------------------------------------------------
    println!("\n[1/6] SetProp...");

    bench_result
        .time_step("SetProp", NUM_SEGMENTS * NUM_PROPS_PER_SEGMENT, || async {
            for s in 1..=NUM_SEGMENTS {
                let segment = format!("segment_{}", s);

                for p in 1..=NUM_PROPS_PER_SEGMENT {
                    let prop_id = format!("s{}_seg{}_p{}", SHARD_IDX, s, p);

                    let mut body = HashMap::new();
                    body.insert("segment".to_string(), json!(segment));
                    body.insert(
                        "area".to_string(),
                        json!(format!("area_{}_{}", SHARD_IDX, s)),
                    );
                    body.insert("property_id".to_string(), json!(prop_id));
                    body.insert("property_type".to_string(), json!("hotel"));
                    body.insert("category".to_string(), json!("midrange"));
                    body.insert("stars".to_string(), json!(3));
                    body.insert("latitude".to_string(), json!(40.7128 + (p as f64) * 0.001));
                    body.insert(
                        "longitude".to_string(),
                        json!(-74.0060 + (p as f64) * 0.001),
                    );
                    body.insert("amenities".to_string(), json!(["wifi", "pool"]));

                    println!("        Creating {}...", prop_id);
                    let _ = client
                        .do_request("SETPROP", &segment, body)
                        .await
                        .map_err(|e| format!("SETPROP {} failed: {}", prop_id, e))?;
                }
            }
            Ok(())
        })
        .await
        .map_err(|e| format!("SetProp failed: {}", e))?;

    // -------------------------------------------------------------------------
    // STEP 2: SetRoomPkg
    // -------------------------------------------------------------------------
    println!("\n[2/6] SetRoomPkg...");

    bench_result
        .time_step(
            "SetRoomPkg",
            NUM_SEGMENTS * NUM_PROPS_PER_SEGMENT * NUM_ROOMS_PER_PROP * NUM_DAYS,
            || async {
                // Generate dates
                let mut dates = Vec::with_capacity(NUM_DAYS);
                let now = Utc::now();
                for i in 0..NUM_DAYS {
                    let date = now + Duration::days(i as i64);
                    dates.push(date.format("%Y-%m-%d").to_string());
                }

                for s in 1..=NUM_SEGMENTS {
                    let segment = format!("segment_{}", s);

                    for p in 1..=NUM_PROPS_PER_SEGMENT {
                        let prop_id = format!("s{}_seg{}_p{}", SHARD_IDX, s, p);

                        for r in 1..=NUM_ROOMS_PER_PROP {
                            let room_type = format!("room{}", r);

                            for date in &dates {
                                let mut body = HashMap::new();
                                body.insert("property_id".to_string(), json!(prop_id));
                                body.insert("room_type".to_string(), json!(room_type));
                                body.insert("date".to_string(), json!(date));
                                body.insert("availability".to_string(), json!(10 + p));
                                body.insert(
                                    "final_price".to_string(),
                                    json!(100 + (p * 10) as i32),
                                );
                                body.insert(
                                    "rate_features".to_string(),
                                    json!(["free_cancellation", "free_wifi"]),
                                );

                                let _ = client
                                    .do_request("SETROOMPKG", &segment, body)
                                    .await
                                    .map_err(|e| {
                                        format!(
                                            "SETROOMPKG {}/{}/{} failed: {}",
                                            prop_id, room_type, date, e
                                        )
                                    })?;
                            }
                        }
                    }
                }
                Ok(())
            },
        )
        .await
        .map_err(|e| format!("SetRoomPkg failed: {}", e))?;

    // -------------------------------------------------------------------------
    // STEP 3: GetPropRoomDay (spot check)
    // -------------------------------------------------------------------------
    println!("\n[3/6] GetPropRoomDay...");

    bench_result
        .time_step("GetPropRoomDay", 1, || async {
            let test_prop = format!("s{}_seg1_p1", SHARD_IDX);
            let room_type = "room1";
            let date = Utc::now().format("%Y-%m-%d").to_string();

            let mut body = HashMap::new();
            body.insert("property_id".to_string(), json!(test_prop));
            body.insert("room_type".to_string(), json!(room_type));
            body.insert("date".to_string(), json!(date));

            let resp = client
                .do_request("GETPROPROOMDAY", "segment_1", body)
                .await?;

            if resp.status != "success" {
                return Err(format!("GETPROPROOMDAY failed: {:?}", resp));
            }
            Ok(())
        })
        .await
        .map_err(|e| format!("GetPropRoomDay failed: {}", e))?;

    // -------------------------------------------------------------------------
    // STEP 4: SearchAvail
    // -------------------------------------------------------------------------
    println!("\n[4/6] SearchAvail...");

    bench_result
        .time_step("SearchAvail", 1, || async {
            let date = Utc::now().format("%Y-%m-%d").to_string();
            let limit: u64 = 100;
            let max_price: u32 = 150;

            let mut body = HashMap::new();
            body.insert("segment".to_string(), json!("segment_1"));
            body.insert("room_type".to_string(), json!("room1"));
            body.insert("date".to_string(), json!([date]));
            body.insert("final_price".to_string(), json!(max_price));
            body.insert("limit".to_string(), json!(limit));

            let resp = client.do_request("SEARCHAVAIL", "segment_1", body).await?;

            if resp.status != "success" {
                return Err(format!("SEARCHAVAIL failed: {:?}", resp));
            }
            Ok(())
        })
        .await
        .map_err(|e| format!("SearchAvail failed: {}", e))?;

    // -------------------------------------------------------------------------
    // STEP 5: Update Availability (Set, Inc, Dec)
    // -------------------------------------------------------------------------
    println!("\n[5/6] Update Availability...");

    bench_result
        .time_step("Update Availability", 3, || async {
            let test_prop = format!("s{}_seg1_p1", SHARD_IDX);
            let room_type = "room1";
            let date = Utc::now().format("%Y-%m-%d").to_string();

            // Set
            let mut body = HashMap::new();
            body.insert("property_id".to_string(), json!(test_prop));
            body.insert("room_type".to_string(), json!(room_type));
            body.insert("date".to_string(), json!(date));
            body.insert("amount".to_string(), json!(20));

            let resp = client.do_request("SETROOMAVL", "segment_1", body).await?;
            if resp.status != "success" {
                return Err(format!("SETROOMAVL failed: {:?}", resp));
            }

            // Inc
            let mut body = HashMap::new();
            body.insert("property_id".to_string(), json!(test_prop));
            body.insert("room_type".to_string(), json!(room_type));
            body.insert("date".to_string(), json!(date));
            body.insert("amount".to_string(), json!(1));

            let resp = client
                .do_request("INCROOMAVL", "segment_1", body.clone())
                .await?;
            if resp.status != "success" {
                return Err(format!("INCROOMAVL failed: {:?}", resp));
            }

            // Dec
            let resp = client.do_request("DECROOMAVL", "segment_1", body).await?;
            if resp.status != "success" {
                return Err(format!("DECROOMAVL failed: {:?}", resp));
            }

            Ok(())
        })
        .await
        .map_err(|e| format!("Update Availability failed: {}", e))?;

    // -------------------------------------------------------------------------
    // STEP 6: Cleanup
    // -------------------------------------------------------------------------
    println!("\n[6/6] Cleanup...");

    bench_result
        .time_step("Cleanup", NUM_SEGMENTS, || async {
            for s in 1..=NUM_SEGMENTS {
                let seg = format!("segment_{}", s);
                let mut body = HashMap::new();
                body.insert("segment".to_string(), json!(seg));

                match client.do_request("DELSEGMENT", &seg, body).await {
                    Ok(_) => println!("        Cleaned up {}", seg),
                    Err(e) => println!("        Warning: Failed to delete {}: {}", seg, e),
                }
            }
            Ok(())
        })
        .await
        .unwrap_or_else(|e| {
            println!("Cleanup had issues: {}", e);
        });

    // -------------------------------------------------------------------------
    // SUMMARY
    // -------------------------------------------------------------------------
    bench_result.print_summary();
    println!("\n✅ All completed successfully!");

    Ok(())
}
