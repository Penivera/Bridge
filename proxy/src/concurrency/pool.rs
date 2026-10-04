use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use crossbeam_deque::{Injector, Steal, Stealer, Worker};
use hyper::server::conn::http1::Builder as ServerBuilder;
use hyper_util::rt::TokioIo;
use tokio::net::TcpStream;
use tokio::sync::Notify;

use crate::concurrency::monitor::{QueueDwellMonitor, DEFAULT_SCALE_UP_THRESHOLD};
use crate::core::enums::ProxyMode;
use crate::Proxy;

/// Default idle cooldown timeout before an auto-spawned dynamic worker exits (15 seconds).
pub const DEFAULT_IDLE_COOLDOWN: Duration = Duration::from_secs(15);

/// An incoming TCP connection pending worker processing.
pub struct ConnectionTask {
    pub stream: TcpStream,
    pub client_addr: SocketAddr,
    pub accepted_at: Instant,
}

pub struct WorkerPoolInner {
    pub injector: Injector<ConnectionTask>,
    pub stealers: RwLock<Vec<(usize, Stealer<ConnectionTask>)>>,
    pub monitor: Mutex<QueueDwellMonitor>,
    pub active_workers: AtomicUsize,
    pub peak_workers: AtomicUsize,
    pub worker_id_seq: AtomicUsize,
    pub max_concurrency: usize,
    pub scale_up_threshold: Duration,
    pub idle_cooldown: Duration,
    pub notify: Notify,
    pub shutdown_notify: Notify,
    pub is_shutdown: AtomicBool,
    pub svc: Proxy,
}

impl WorkerPoolInner {
    /// Attempts to scale up the worker pool by spawning a new dynamic worker task.
    pub fn try_scale_up(self: &Arc<Self>) -> bool {
        if self.is_shutdown.load(Ordering::Relaxed) {
            return false;
        }

        let mut current = self.active_workers.load(Ordering::Acquire);
        while current < self.max_concurrency {
            match self.active_workers.compare_exchange_weak(
                current,
                current + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    let new_count = current + 1;
                    self.peak_workers.fetch_max(new_count, Ordering::Relaxed);
                    spawn_worker_task(self.clone(), false);
                    return true;
                }
                Err(actual) => current = actual,
            }
        }
        false
    }
}

/// Adaptive elastic worker pool managing work-stealing deques and autoscaling.
#[derive(Clone)]
pub struct ElasticWorkerPool {
    inner: Arc<WorkerPoolInner>,
}

impl ElasticWorkerPool {
    /// Creates and initializes the elastic worker pool with baseline active worker 1.
    pub fn new(svc: Proxy, max_concurrency: usize) -> Self {
        Self::with_options(
            svc,
            max_concurrency,
            DEFAULT_SCALE_UP_THRESHOLD,
            DEFAULT_IDLE_COOLDOWN,
        )
    }

    /// Creates and initializes the elastic worker pool with custom threshold and cooldown options.
    pub fn with_options(
        svc: Proxy,
        max_concurrency: usize,
        scale_up_threshold: Duration,
        idle_cooldown: Duration,
    ) -> Self {
        let max_concurrency = std::cmp::max(1, max_concurrency);
        let inner = Arc::new(WorkerPoolInner {
            injector: Injector::new(),
            stealers: RwLock::new(Vec::new()),
            monitor: Mutex::new(QueueDwellMonitor::default()),
            active_workers: AtomicUsize::new(1),
            peak_workers: AtomicUsize::new(1),
            worker_id_seq: AtomicUsize::new(0),
            max_concurrency,
            scale_up_threshold,
            idle_cooldown,
            notify: Notify::new(),
            shutdown_notify: Notify::new(),
            is_shutdown: AtomicBool::new(false),
            svc,
        });

        // Spawn baseline active worker 1
        spawn_worker_task(inner.clone(), true);

        Self { inner }
    }

    /// Returns the number of currently active worker tasks.
    pub fn active_workers(&self) -> usize {
        self.inner.active_workers.load(Ordering::Acquire)
    }

    /// Returns the highest number of concurrent worker tasks spawned.
    pub fn peak_workers(&self) -> usize {
        self.inner.peak_workers.load(Ordering::Acquire)
    }

    /// Returns the maximum concurrency ceiling.
    pub fn max_concurrency(&self) -> usize {
        self.inner.max_concurrency
    }

    /// Returns the average dwell duration of the last 20 requests.
    pub fn average_dwell(&self) -> Duration {
        let mon = self.inner.monitor.lock().unwrap();
        mon.average_dwell()
    }

    /// Dispatches an accepted TCP stream to the ingress work-stealing queue.
    pub fn dispatch(&self, stream: TcpStream, client_addr: SocketAddr) {
        let task = ConnectionTask {
            stream,
            client_addr,
            accepted_at: Instant::now(),
        };

        self.inner.injector.push(task);
        self.inner.notify.notify_one();

        // Check if there is queue backlog and capacity to scale up
        let active = self.inner.active_workers.load(Ordering::Acquire);
        if active < self.inner.max_concurrency {
            let should_scale = {
                let mon = self.inner.monitor.lock().unwrap();
                mon.should_scale_up(self.inner.scale_up_threshold) || !self.inner.injector.is_empty()
            };
            if should_scale {
                self.inner.try_scale_up();
            }
        }
    }

    /// Signals all workers to shut down.
    pub fn shutdown(&self) {
        self.inner.is_shutdown.store(true, Ordering::Release);
        self.inner.shutdown_notify.notify_waiters();
        self.inner.notify.notify_waiters();
    }
}

fn spawn_worker_task(inner: Arc<WorkerPoolInner>, is_baseline: bool) {
    let worker_id = inner.worker_id_seq.fetch_add(1, Ordering::Relaxed);
    let worker = Worker::new_lifo();
    let stealer = worker.stealer();

    {
        let mut guard = inner.stealers.write().unwrap();
        guard.push((worker_id, stealer));
    }

    tracing::debug!(
        worker_id,
        is_baseline,
        active_workers = inner.active_workers.load(Ordering::Relaxed),
        "spawned worker task"
    );

    tokio::spawn(async move {
        worker_loop(inner, worker, worker_id, is_baseline).await;
    });
}

async fn worker_loop(
    inner: Arc<WorkerPoolInner>,
    local: Worker<ConnectionTask>,
    worker_id: usize,
    is_baseline: bool,
) {
    loop {
        if inner.is_shutdown.load(Ordering::Relaxed) {
            break;
        }

        let stealers = {
            let guard = inner.stealers.read().unwrap();
            guard.clone()
        };

        let task = find_task(&local, &inner.injector, &stealers, worker_id);

        match task {
            Some(task) => {
                let dwell = task.accepted_at.elapsed();
                {
                    let mut mon = inner.monitor.lock().unwrap();
                    mon.record(dwell);
                    if mon.should_scale_up(inner.scale_up_threshold) {
                        inner.try_scale_up();
                    }
                }

                match inner.svc.config.mode {
                    ProxyMode::Direct => {
                        let io = TokioIo::new(task.stream);
                        if let Err(err) = ServerBuilder::new()
                            .preserve_header_case(true)
                            .title_case_headers(true)
                            .serve_connection(io, inner.svc.clone())
                            .await
                        {
                            tracing::debug!(worker_id, "error serving connection: {err}");
                        }
                    }
                    ProxyMode::Handoff => {
                        let default_routing = inner.svc.config.routing;
                        let redirect_http = inner.svc.config.redirect_http;
                        let send_proxy_protocol = inner.svc.config.proxy_protocol;
                        let registry = inner.svc.registry.clone();
                        if let Err(err) = crate::handoff::handle_handoff_connection(
                            task.stream,
                            task.client_addr,
                            registry,
                            default_routing,
                            redirect_http,
                            send_proxy_protocol,
                        )
                        .await
                        {
                            tracing::debug!(worker_id, client = %task.client_addr, "handoff error: {err}");
                        }
                    }
                    ProxyMode::Managed => {}
                }
            }
            None => {
                if is_baseline {
                    // Baseline Worker 1 waits indefinitely for tasks or shutdown
                    tokio::select! {
                        _ = inner.shutdown_notify.notified() => break,
                        _ = inner.notify.notified() => {}
                    }
                } else {
                    // Dynamic workers wait up to idle_cooldown before graceful exit
                    tokio::select! {
                        _ = inner.shutdown_notify.notified() => break,
                        res = tokio::time::timeout(inner.idle_cooldown, inner.notify.notified()) => {
                            if res.is_err() {
                                // Cooldown elapsed: verify queue is empty before exiting
                                if inner.injector.is_empty() && local.is_empty() {
                                    tracing::debug!(
                                        worker_id,
                                        "dynamic worker idle cooldown expired, gracefully exiting"
                                    );
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Clean up worker registration
    {
        let mut guard = inner.stealers.write().unwrap();
        guard.retain(|&(id, _)| id != worker_id);
    }
    inner.active_workers.fetch_sub(1, Ordering::Release);
}

fn find_task(
    local: &Worker<ConnectionTask>,
    injector: &Injector<ConnectionTask>,
    stealers: &[(usize, Stealer<ConnectionTask>)],
    worker_id: usize,
) -> Option<ConnectionTask> {
    // 1. Pop from local worker queue (LIFO for cache locality)
    if let Some(task) = local.pop() {
        return Some(task);
    }

    // 2. Steal batch from global injector
    loop {
        match injector.steal_batch_and_pop(local) {
            Steal::Success(task) => return Some(task),
            Steal::Empty => break,
            Steal::Retry => continue,
        }
    }

    // 3. Work-steal from other active workers (FIFO from top)
    for &(id, ref stealer) in stealers {
        if id == worker_id {
            continue;
        }
        loop {
            match stealer.steal() {
                Steal::Success(task) => return Some(task),
                Steal::Empty => break,
                Steal::Retry => continue,
            }
        }
    }

    None
}
