pub mod wireguard;

pub use wireguard::{
    derive_wireguard_public_key, generate_wireguard_keypair, parse_wireguard_key,
    WireGuardDevice, WireGuardPeer,
};
