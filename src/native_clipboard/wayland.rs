//! Clipboard access through the focused application's Wayland connection.
//! Unlike data-control, wl_data_device is available on ordinary GNOME sessions.
mod state;
mod transfer;
mod worker;

use crate::{Result, invalid};
use quickgui::{ClipboardEntry, ClipboardError, ClipboardItem, ClipboardProvider};
use raw_window_handle::{HasDisplayHandle, RawDisplayHandle};
use sctk::reexports::calloop::channel;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;
use winit::event_loop::OwnedDisplayHandle;

#[derive(Clone)]
pub struct WaylandClipboard(Arc<Inner>);

struct Inner {
    sender: channel::Sender<Command>,
    gate: Mutex<()>,
    worker: Mutex<Option<std::thread::JoinHandle<()>>>,
}

#[derive(Clone, Copy)]
enum Kind {
    Image,
    Text,
}

#[derive(Clone)]
struct Payload {
    mime: String,
    bytes: Arc<Vec<u8>>,
}

enum Command {
    Read(Kind, mpsc::Sender<Result<Option<Payload>>>),
    Write(Option<Payload>, mpsc::Sender<Result<()>>),
    Exit,
}

impl WaylandClipboard {
    /// The owned display keeps the foreign Wayland connection alive until its
    /// clipboard event queue has stopped. X11 continues using its native backend.
    pub fn new(display: OwnedDisplayHandle) -> Result<Option<Self>> {
        if !matches!(
            display
                .display_handle()
                .map_err(|e| invalid(e.to_string()))?
                .as_raw(),
            RawDisplayHandle::Wayland(_)
        ) {
            return Ok(None);
        }
        let (sender, receiver) = channel::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        let worker = std::thread::Builder::new()
            .name("wayland-clipboard".into())
            .spawn(move || {
                if let Err(error) = worker::run(display, receiver, &ready_tx) {
                    eprintln!("Wayland clipboard worker stopped: {error}");
                    let _ = ready_tx.send(Err(error));
                }
            })?;
        let clipboard = Self(Arc::new(Inner {
            sender,
            gate: Mutex::new(()),
            worker: Mutex::new(Some(worker)),
        }));
        ready_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| {
                invalid("The Wayland clipboard did not initialize. Restart the editor and retry.")
            })??;
        Ok(Some(clipboard))
    }

    pub fn read_image(&self) -> Result<Option<Vec<u8>>> {
        self.read_kind(Kind::Image)
            .map(|value| value.map(|payload| Arc::unwrap_or_clone(payload.bytes)))
    }

    fn read_kind(&self, kind: Kind) -> Result<Option<Payload>> {
        let _guard = self.0.gate.lock().map_err(|_| {
            invalid("The clipboard reader stopped unexpectedly. Restart the editor.")
        })?;
        let (tx, rx) = mpsc::channel();
        self.0
            .sender
            .send(Command::Read(kind, tx))
            .map_err(|_| invalid("The Wayland clipboard connection closed. Restart the editor."))?;
        rx.recv_timeout(Duration::from_secs(6)).map_err(|_| invalid("The clipboard transfer timed out. Copy the content again and retry; the document is unchanged."))?
    }

    fn write_payload(&self, payload: Option<Payload>) -> Result<()> {
        let _guard = self.0.gate.lock().map_err(|_| {
            invalid("The clipboard writer stopped unexpectedly. Restart the editor.")
        })?;
        let (tx, rx) = mpsc::channel();
        self.0
            .sender
            .send(Command::Write(payload, tx))
            .map_err(|_| invalid("The Wayland clipboard connection closed. Restart the editor."))?;
        rx.recv_timeout(Duration::from_secs(6)).map_err(|_| {
            invalid(
                "The clipboard write timed out. Copy the content again; pixels have not been cut.",
            )
        })?
    }
}

impl ClipboardProvider for WaylandClipboard {
    fn read(&self) -> std::result::Result<Option<ClipboardItem>, ClipboardError> {
        self.read_kind(Kind::Text)
            .map_err(platform_error)?
            .map(|payload| {
                let text =
                    std::str::from_utf8(&payload.bytes).map_err(|_| ClipboardError::InvalidText)?;
                ClipboardItem::new_string(text)
            })
            .transpose()
    }

    fn write(&self, item: ClipboardItem) -> std::result::Result<(), ClipboardError> {
        let payload = if item.is_empty() {
            None
        } else if let Some(image) = item.entries().iter().find_map(|entry| match entry {
            ClipboardEntry::Image(image) => Some(image),
            _ => None,
        }) {
            Some(Payload {
                mime: image.format().mime_type().into(),
                bytes: Arc::new(image.bytes().to_vec()),
            })
        } else if let Some(text) = item.text() {
            Some(Payload {
                mime: "text/plain;charset=utf-8".into(),
                bytes: Arc::new(text.as_bytes().to_vec()),
            })
        } else {
            return Err(ClipboardError::Platform(
                "The clipboard item contains neither image pixels nor text.".into(),
            ));
        };
        self.write_payload(payload).map_err(platform_error)
    }
}

fn platform_error(error: crate::Error) -> ClipboardError {
    ClipboardError::Platform(error.to_string().into())
}

impl Drop for Inner {
    fn drop(&mut self) {
        let _ = self.sender.send(Command::Exit);
        if let Ok(worker) = self.worker.get_mut()
            && let Some(worker) = worker.take()
        {
            // Never block shutdown on a damaged display connection. The worker
            // retains the owned display until it finishes its own cleanup.
            if worker.is_finished() {
                let _ = worker.join();
            }
        }
    }
}
