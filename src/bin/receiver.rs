//! Long-running receiver: listens for WebSocket connections and shows each
//! incoming notice as a Windows toast notification.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result};
use base64::Engine;
use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;
use windows_notice::{Ack, DEFAULT_PORT, Notice};

/// Wait for notices from `sender` and display them as Windows toasts.
#[derive(Parser)]
struct Args {
    /// Address to listen on. `0.0.0.0` accepts connections from other machines.
    #[arg(short, long, default_value_t = format!("0.0.0.0:{DEFAULT_PORT}"))]
    bind: String,
}

/// Counter for unique image file names within this run.
static IMAGE_COUNTER: AtomicU64 = AtomicU64::new(0);

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let image_dir = std::env::temp_dir().join("windows-notice");
    // Images from a previous run are no longer referenced by any toast we care about.
    let _ = std::fs::remove_dir_all(&image_dir);
    std::fs::create_dir_all(&image_dir)?;

    toast::register()?;

    let listener = TcpListener::bind(&args.bind).await.with_context(|| format!("binding {}", args.bind))?;
    println!("Listening on ws://{}", listener.local_addr()?);

    loop {
        let (stream, peer) = listener.accept().await?;
        let image_dir = image_dir.clone();
        tokio::spawn(async move {
            if let Err(e) = handle(stream, image_dir).await {
                eprintln!("[{peer}] {e:#}");
            }
        });
    }
}

async fn handle(stream: TcpStream, image_dir: PathBuf) -> Result<()> {
    let peer = stream.peer_addr()?;
    let mut ws = tokio_tungstenite::accept_async(stream).await?;

    while let Some(msg) = ws.next().await {
        let text = match msg? {
            Message::Text(text) => text,
            Message::Close(_) => break,
            _ => continue,
        };
        let result = show(&text, &image_dir);
        match &result {
            Ok(()) => println!("[{peer}] shown"),
            Err(e) => eprintln!("[{peer}] failed: {e:#}"),
        }
        let ack = Ack { ok: result.is_ok(), error: result.err().map(|e| format!("{e:#}")) };
        ws.send(Message::text(serde_json::to_string(&ack)?)).await?;
    }
    Ok(())
}

fn show(text: &str, image_dir: &std::path::Path) -> Result<()> {
    let notice: Notice = serde_json::from_str(text).context("invalid notice")?;

    let image_path = match notice.image {
        Some(image) => {
            let bytes = base64::engine::general_purpose::STANDARD.decode(&image.data).context("invalid image data")?;
            // Only keep alphanumeric extension chars so a sender can't steer the file path.
            let ext: String = image.extension.chars().filter(char::is_ascii_alphanumeric).collect();
            let n = IMAGE_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = image_dir.join(format!("{n}.{ext}"));
            std::fs::write(&path, bytes)?;
            Some(path)
        }
        None => None,
    };

    let title = notice.title.as_deref().unwrap_or("Notice");
    toast::show(title, &notice.message, image_path.as_deref())
}

#[cfg(windows)]
mod toast {
    use std::path::Path;

    use anyhow::Result;
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
    use windows::core::HSTRING;

    /// AppUserModelID that toasts are attributed to. Unpackaged apps need one
    /// registered so Windows knows what name to show in the toast header.
    const AUMID: &str = "WindowsNotice.Receiver";
    const DISPLAY_NAME: &str = "Windows Notice";

    /// Registers the AUMID under HKCU (no admin needed). This is enough for
    /// Windows 10/11 to accept toasts from an unpackaged executable.
    pub fn register() -> Result<()> {
        let key = windows_registry::CURRENT_USER.create(format!(r"Software\Classes\AppUserModelId\{AUMID}"))?;
        key.set_string("DisplayName", DISPLAY_NAME)?;
        Ok(())
    }

    pub fn show(title: &str, message: &str, image: Option<&Path>) -> Result<()> {
        let image_xml = match image {
            Some(path) => format!(r#"<image src="{}"/>"#, escape(&path.to_string_lossy())),
            None => String::new(),
        };
        let xml = format!(
            r#"<toast><visual><binding template="ToastGeneric"><text>{}</text><text>{}</text>{image_xml}</binding></visual></toast>"#,
            escape(title),
            escape(message),
        );

        let doc = XmlDocument::new()?;
        doc.LoadXml(&HSTRING::from(xml))?;
        let toast = ToastNotification::CreateToastNotification(&doc)?;
        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(AUMID))?.Show(&toast)?;
        Ok(())
    }

    fn escape(s: &str) -> String {
        s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
    }
}

/// Lets the crate build on non-Windows machines (where only `sender` is useful).
#[cfg(not(windows))]
mod toast {
    use std::path::Path;

    pub fn register() -> anyhow::Result<()> {
        eprintln!("Not on Windows: notices will be printed instead of shown as toasts.");
        Ok(())
    }

    pub fn show(title: &str, message: &str, image: Option<&Path>) -> anyhow::Result<()> {
        println!("{title}: {message} {image:?}");
        Ok(())
    }
}
