pub mod wireguard;

pub use wireguard::{
    generate_wireguard_keypair, parse_wireguard_key, WireGuardDevice, WireGuardPeer,
};
