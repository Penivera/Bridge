pub mod docker;

pub use docker::{
    parse_docker_labels, parse_traefik_rule, DiscoveredServiceInfo, DockerDiscovery,
};
