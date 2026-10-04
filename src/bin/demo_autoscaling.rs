use std::sync::Arc;
use std::time::{Duration, Instant};

use clap::Parser;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinSet;
use tracing_subscriber::EnvFilter;

use proxy::concurrency::ElasticWorkerPool;
use proxy::core::config::auto_detect_max_concurrency;
use proxy::{Proxy, ProxyConfig};
use registry::DomainRegistry;

#[derive(Parser, Debug)]
#[command(
    name = "demo-autoscaling",
    about = "Interactive demonstration and stress verification of Bridge's adaptive work-stealing concurrency autoscaler."
)]
struct Args {
    /// Maximum concurrency ceiling for worker tasks (or 0 to auto-detect from CPU count)
    #[arg(short = 'c', long, default_value_t = 8)]
    max_concurrency: usize,

    /// Number of burst client connections to dispatch
    #[arg(short = 'n', long, default_value_t = 30)]
    requests: usize,

    /// Idle cooldown in seconds before dynamic workers gracefully exit
    #[arg(short = 't', long, default_value_t = 3)]
    cooldown_secs: u64,

    /// Ingress port to bind the proxy listener (0 chooses a random available port)
    #[arg(short = 'p', long, default_value_t = 0)]
    port: u16,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // Initialize structured logging with formatted timestamps and log levels
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,proxy::concurrency=debug")),
        )
        .with_target(false)
        .init();

    let max_concurrency = if args.max_concurrency == 0 {
        auto_detect_max_concurrency()
    } else {
        args.max_concurrency
    };

    println!("\n╔══════════════════════════════════════════════════════════════════╗");
    println!("║       BRIDGE ADAPTIVE CONCURRENCY AUTOSCALING DEMO               ║");
    println!("╚══════════════════════════════════════════════════════════════════╝\n");

    // 1. Bind listener
    let listener = TcpListener::bind(format!("127.0.0.1:{}", args.port)).await?;
    let listen_addr = listener.local_addr()?;

    println!("Configuration Parameters:");
    println!("  • Ingress Address:    {listen_addr}");
    println!("  • Concurrency Limit:  {max_concurrency} worker tasks");
    println!("  • Traffic Burst:      {} concurrent requests", args.requests);
    println!("  • Idle Cooldown:      {}s before worker reaping", args.cooldown_secs);
    println!("  • Latency Trigger:    5ms average dwell threshold\n");

    // 2. Initialize Proxy and ElasticWorkerPool
    let mut config = ProxyConfig::default();
    config.mode = proxy::core::enums::ProxyMode::Direct;
    config.max_concurrency = max_concurrency;

    let registry = Arc::new(DomainRegistry::new());
    let proxy = Proxy::new(Arc::new(config), registry);

    let pool = Arc::new(ElasticWorkerPool::with_options(
        proxy,
        max_concurrency,
        Duration::from_millis(5),
        Duration::from_secs(args.cooldown_secs),
    ));

    // Spawn TCP Acceptor Loop (Zero-blocking ingress dispatch)
    let pool_dispatch = pool.clone();
    let acceptor_task = tokio::spawn(async move {
        while let Ok((stream, client_addr)) = listener.accept().await {
            pool_dispatch.dispatch(stream, client_addr);
        }
    });

    // ── PHASE 1: Baseline Idle Footprint ────────────────────────────
    println!("─── [PHASE 1] Initializing Baseline Footprint ─────────────────────");
    tokio::time::sleep(Duration::from_millis(50)).await;
    let initial_workers = pool.active_workers();
    println!("  Active Workers:       {initial_workers} (Baseline Worker 0)");
    assert_eq!(
        initial_workers, 1,
        "Pool must start with exactly 1 baseline worker"
    );
    println!("  Memory / CPU Footprint: Minimal (near-zero idle load)\n");

    // ── PHASE 2: Traffic Burst Simulation ───────────────────────────
    println!("─── [PHASE 2] Simulating Burst Traffic ({} requests) ─────────", args.requests);
    println!("  Dispatching concurrent client connections to {listen_addr}...");

    let start_time = Instant::now();
    let mut clients = JoinSet::new();

    for i in 0..args.requests {
        clients.spawn(async move {
            let mut stream = TcpStream::connect(listen_addr).await.expect("connection failed");
            let req = format!(
                "GET /test-{} HTTP/1.1\r\nHost: demo.test\r\nConnection: close\r\n\r\n",
                i
            );
            stream.write_all(req.as_bytes()).await.expect("write failed");

            let mut resp = Vec::new();
            stream.read_to_end(&mut resp).await.expect("read failed");
            assert!(
                resp.starts_with(b"HTTP/1.1 200 OK"),
                "expected 200 OK from proxy"
            );
        });
    }

    let mut successful_requests = 0;
    while let Some(res) = clients.join_next().await {
        res?;
        successful_requests += 1;
    }

    let elapsed = start_time.elapsed();
    let peak = pool.peak_workers();
    println!("  Completed:            {successful_requests}/{} requests in {:.2?}", args.requests, elapsed);
    println!("  Peak Active Workers:  {peak} (Auto-scaled from 1 to {peak} based on queue dwell)");
    println!("  Average Dwell Latency:{:?}", pool.average_dwell());
    assert!(
        peak > 1,
        "Worker pool should have auto-scaled up under burst load"
    );
    println!("  ✓ Scale-up verified successfully!\n");

    // ── PHASE 3: Idle Cooldown & Reaper Scale-Down ──────────────────
    println!("─── [PHASE 3] Cooldown & Dynamic Worker Reaping ─────────────────");
    println!("  Traffic stopped. Monitoring worker idle cooldown ({}s)...", args.cooldown_secs);

    let wait_start = Instant::now();
    let max_wait = Duration::from_secs(args.cooldown_secs + 2);

    while wait_start.elapsed() < max_wait {
        let current = pool.active_workers();
        println!(
            "  [{:4.1}s] Active Workers: {} (Queue empty: idle workers counting down...)",
            wait_start.elapsed().as_secs_f32(),
            current
        );
        if current == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    let final_workers = pool.active_workers();
    println!("\n  Final Active Workers: {final_workers} (Baseline Worker maintained)");
    assert_eq!(
        final_workers, 1,
        "Dynamic workers must gracefully exit, returning to baseline 1"
    );
    println!("  ✓ Scale-down and idle reaper verified successfully!\n");

    // ── SUMMARY REPORT ──────────────────────────────────────────────
    println!("╔══════════════════════════════════════════════════════════════════╗");
    println!("║                    AUTOSCALING VERIFICATION REPORT              ║");
    println!("╠══════════════════════════════════════════════════════════════════╣");
    println!("║  Baseline active workers:            1                           ║");
    println!("║  Burst requests dispatched:          {:2}                          ║", args.requests);
    println!("║  Peak concurrent workers spawned:    {:2}                          ║", peak);
    println!("║  Worker ceiling (max_concurrency):   {:2}                          ║", max_concurrency);
    println!("║  Idle cooldown timeout:              {}s                          ║", args.cooldown_secs);
    println!("║  Workers reaped after cooldown:      {}                          ║", peak - final_workers);
    println!("║  Final active workers:               1                           ║");
    println!("║  Requests completed successfully:    {:2}/{:2}                      ║", successful_requests, args.requests);
    println!("║  Status:                             ALL CHECKS PASSED           ║");
    println!("╚══════════════════════════════════════════════════════════════════╝\n");

    // Graceful cleanup
    acceptor_task.abort();
    pool.shutdown();

    Ok(())
}
