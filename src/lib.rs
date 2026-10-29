//! Wire protocol shared by the `sender` and `receiver` binaries.
//!
//! The sender opens a WebSocket to the receiver, sends one [`Notice`] as a JSON
//! text frame, and waits for a single [`Ack`] in reply.

use serde::{Deserialize, Serialize};

/// Default port the receiver listens on.
pub const DEFAULT_PORT: u16 = 9001;

/// A notification to display on the receiving machine.
#[derive(Debug, Serialize, Deserialize)]
pub struct Notice {
    /// Bold first line of the toast. The receiver uses a default if absent.
    pub title: Option<String>,
    /// Body text of the toast.
    pub message: String,
    /// Image bytes travel with the message, since the path only exists on the sender.
    pub image: Option<Image>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Image {
    /// File extension (e.g. `png`), used so Windows can identify the format.
    pub extension: String,
    /// Base64-encoded file contents.
    pub data: String,
}

/// The receiver's reply to a [`Notice`].
#[derive(Debug, Serialize, Deserialize)]
pub struct Ack {
    pub ok: bool,
    pub error: Option<String>,
}
