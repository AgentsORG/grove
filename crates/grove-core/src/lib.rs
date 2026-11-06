//! # grove-core
//!
//! The engine behind Grove: source control for people and agents, with one
//! graph underneath.
//!
//! - The **program graph** is the record. Zero files are split into
//!   declarations; every declaration has a stable id and is addressed by the
//!   SHA-256 of its source. Edges come from references and are never stored.
//! - A **change** is a list of ops on that graph, the intent behind it, who
//!   made it (and for whom), and its **footprint**: what it writes, what its
//!   renames rewrite, and what it reads.
//! - **Landing** replays a change on whatever reached main since it began.
//!   Changes with separate footprints land together; one that depends on
//!   another is ordered after it; two that set the same declaration differently
//!   become a saved conflict. Checks run on the combined result first.
//! - Every operation is in an **operation log** and can be undone.

pub mod check;
pub mod diff;
pub mod git;
pub mod graph;
pub mod hash;
pub mod history;
pub mod model;
pub mod remote;
pub mod repo;
pub mod shell;
pub mod source;
pub mod store;
pub mod textdiff;
pub mod workdir;

pub use check::CheckSpec;
pub use graph::{Edge, EdgeKind, View};
pub use hash::Hash;
pub use model::*;
pub use remote::{Fetched, Folder, Object, OpenChange, Proposal, PushReport, Remote, RemoteRefs};
pub use repo::{mode_word, short, Config, LandOutcome, Refs, Repo, Workspace, DEFAULT_WORKSPACE};
