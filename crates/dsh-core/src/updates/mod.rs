//! Which dsh versions exist, and installing one.
//!
//! Three jobs that only share a subject: [`registry`] reads what npm has
//! published and splits it into channels, [`install`] builds the command that
//! installs a version, and [`dismiss`] remembers which versions the user asked
//! not to be reminded about.

pub mod dismiss;
pub mod install;
pub mod registry;

pub use dismiss::DismissStore;
pub use install::{cmd_command_line, cmd_quote, install_argv, install_command, InstallCommand};
pub use registry::{
    build_report, channel_of, check, is_newer, registry_url, stability_rank, ChannelListing,
    RegistryDoc, UpdateReport, VersionEntry,
};

/// Where released versions are read from.
pub const DEFAULT_REGISTRY_URL: &str = "https://registry.npmjs.org/@deepseek-ai/dsh";
/// How many versions of each channel the report carries.
pub const VERSIONS_PER_CHANNEL: usize = 6;

/// The npm package this shell wraps.
pub const PACKAGE: &str = "@deepseek-ai/dsh";

/// Channel identifiers, in the order they are shown.
pub const CHANNELS: [(&str, &str); 3] = [("stable", "正式版"), ("rc", "RC"), ("alpha", "Alpha")];
