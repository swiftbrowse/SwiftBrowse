//! SwiftBrowse shared automation core.
//!
//! Every interface (MCP, CLI, SDKs) sends a [`Request`] to a [`SessionManager`]
//! and receives an [`ActionResult`]. Browser engines plug in through
//! [`Backend`]; the core owns locator resolution, policy, waiting,
//! verification, recovery limits and evidence.

pub mod backend;
pub mod capability;
pub mod error;
pub mod locator;
pub mod manager;
pub mod mock;
pub mod model;
pub mod policy;
pub mod request;
pub mod result;
pub mod session;

pub use backend::{Backend, BackendFactory, SessionOptions};
pub use capability::{Capability, CapabilityReport, Support};
pub use error::{CoreError, ErrorKind, Result};
pub use locator::{Locator, Target};
pub use manager::SessionManager;
pub use policy::Policy;
pub use request::{Condition, Request};
pub use result::ActionResult;
pub use session::Session;
