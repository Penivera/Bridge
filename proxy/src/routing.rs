//! Routing module for the Bridge proxy layer.
//!
//! Exposes domain registry types and routing table primitives used to
//! map inbound domain requests to destination nodes and endpoints.

pub use registry::{DomainRegistry, Node, Route};
