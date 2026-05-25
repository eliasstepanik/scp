use once_cell::sync::Lazy;
use prometheus::{CounterVec, GaugeVec, Opts};

/// Total stdio process spawns, labeled by server_name.
pub static SCP_POOL_SPAWNS_TOTAL: Lazy<CounterVec> = Lazy::new(|| {
    let counter = CounterVec::new(
        Opts::new(
            "scp_pool_spawns_total",
            "Total stdio process spawns by server",
        ),
        &["server_name"],
    )
    .expect("failed to create scp_pool_spawns_total — this is a bug in SCP startup");
    prometheus::default_registry()
        .register(Box::new(counter.clone()))
        .expect("failed to register scp_pool_spawns_total — this is a bug in SCP startup");
    counter
});

/// Total stdio process crashes (receive_loop errors), labeled by server_name.
pub static SCP_POOL_CRASHES_TOTAL: Lazy<CounterVec> = Lazy::new(|| {
    let counter = CounterVec::new(
        Opts::new(
            "scp_pool_crashes_total",
            "Total stdio process crashes by server",
        ),
        &["server_name"],
    )
    .expect("failed to create scp_pool_crashes_total — this is a bug in SCP startup");
    prometheus::default_registry()
        .register(Box::new(counter.clone()))
        .expect("failed to register scp_pool_crashes_total — this is a bug in SCP startup");
    counter
});

/// Active stdio processes (1 = running, 0 = dead), labeled by server_name.
pub static SCP_POOL_ACTIVE_PROCESSES: Lazy<GaugeVec> = Lazy::new(|| {
    let gauge = GaugeVec::new(
        Opts::new(
            "scp_pool_active_processes",
            "Active stdio backend processes by server (1=running, 0=dead)",
        ),
        &["server_name"],
    )
    .expect("failed to create scp_pool_active_processes — this is a bug in SCP startup");
    prometheus::default_registry()
        .register(Box::new(gauge.clone()))
        .expect("failed to register scp_pool_active_processes — this is a bug in SCP startup");
    gauge
});
