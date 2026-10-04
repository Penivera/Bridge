pub mod concurrency;
pub mod core;
pub mod error;
pub mod handoff;
pub mod managed;
pub mod routing;
pub mod server;
pub mod shutdown;
pub mod transport;
pub mod udp;

pub use core::config::{
    detect_traefik_dynamic_path, detect_traefik_dynamic_path_from, ManagedConfig, ProxyConfig,
    UdpServiceConfig,
};
pub use error::ProxyError;
pub use handoff::{
    encode_proxy_v2, encode_proxy_v2_with_protocol, handle_handoff_connection, parse_proxy_v2,
    ProxyCommand, ProxyV2Header, TransportProtocol, PROXY_V2_PREFIX,
};
pub use managed::{
    generate_traefik_config, is_local_route, sanitize_traefik_name, sync_dynamic_config,
    sync_traefik_file, traefik_config_to_yaml, TraefikConfig, TraefikHttp, TraefikLoadBalancer,
    TraefikRouter, TraefikServer, TraefikService, TraefikTls,
};
pub use server::Proxy;
pub use shutdown::{RuntimeState, ShutdownAck, ShutdownCoordinator, ShutdownReason};
pub use udp::{run_udp_service, run_udp_services, run_udp_services_with_shutdown, UdpListener};

