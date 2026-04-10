#![cfg(feature = "agave-unstable-api")]
// Activate some of the Rust 2024 lints to make the future migration easier.
#![warn(if_let_rescope)]
#![warn(keyword_idents_2024)]
#![warn(rust_2024_incompatible_pat)]
#![warn(tail_expr_drop_order)]
#![warn(unsafe_attr_outside_unsafe)]
#![warn(unsafe_op_in_unsafe_fn)]

#[cfg(target_os = "linux")]
pub mod device;
#[cfg(target_os = "linux")]
pub mod gre;
#[cfg(target_os = "linux")]
pub(crate) mod lpm;
#[cfg(target_os = "linux")]
pub mod netlink;
#[cfg(target_os = "linux")]
pub mod packet;
#[cfg(target_os = "linux")]
mod program;
#[cfg(target_os = "linux")]
pub mod route;
#[cfg(target_os = "linux")]
pub mod route_monitor;
#[cfg(target_os = "linux")]
pub mod socket;
#[cfg(target_os = "linux")]
pub mod tx_loop;
#[cfg(target_os = "linux")]
pub mod umem;

pub mod transmitter;

#[cfg(target_os = "linux")]
pub use program::load_xdp_program;
use std::{io, net::Ipv4Addr};

/// Returns the IPv4 address of the specified network interface.
///
/// If the interface is part of a bonded interface, returns the master's IPv4 address.
#[cfg(target_os = "linux")]
pub fn interface_ipv4(interface: &str) -> Result<Ipv4Addr, io::Error> {
    if let Some(ip) = crate::transmitter::master_ip_if_bonded(interface) {
        Ok(ip)
    } else {
        crate::device::NetworkDevice::new(interface)?.ipv4_addr()
    }
}

#[cfg(not(target_os = "linux"))]
pub fn interface_ipv4(_interface: &str) -> Result<Ipv4Addr, io::Error> {
    unimplemented!()
}

/// Returns the IPv4 address of the device associated with the default route.
#[cfg(target_os = "linux")]
pub fn default_device_ipv4() -> Result<Ipv4Addr, io::Error> {
    crate::device::NetworkDevice::new_from_default_route()?.ipv4_addr()
}

#[cfg(not(target_os = "linux"))]
pub fn default_device_ipv4() -> Result<Ipv4Addr, io::Error> {
    unimplemented!()
}
