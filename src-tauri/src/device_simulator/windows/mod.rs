//! Testable Windows network planning primitives.
//!
//! Native enumeration and mutation are deliberately kept behind traits. The
//! planner is platform-independent so CIDR, ownership, and cleanup rules can
//! be verified without changing the host network.

pub mod elevation;
pub mod firewall;
pub mod interfaces;
pub mod ip_alias;
pub mod named_pipe;
