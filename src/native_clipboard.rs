//! Read original encoded images, preserving profiles and orientation that an RGBA
//! clipboard adapter would discard. Call on a worker, never on the UI thread.
pub mod wayland;
mod x11;

use crate::{Result, invalid};
use std::{
    io::Read,
    os::fd::AsFd,
    time::{Duration, Instant},
};
use wl_clipboard_rs::paste::{self, ClipboardType, MimeType, Seat};

const MAX_BYTES: usize = 400_000_000;
const TIMEOUT: Duration = Duration::from_secs(5);

fn supported(mime: &str) -> bool {
    matches!(
        mime,
        "image/png" | "image/tiff" | "image/jpeg" | "image/webp"
    )
}

pub fn read_image() -> Result<Option<Vec<u8>>> {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        match read_wayland() {
            // Match the fallback used by QuickGUI/arboard on compositors without
            // data-control. Once a transfer starts, do not silently change sources.
            Err(WaylandError::Unavailable(error)) => {
                if std::env::var_os("DISPLAY").is_none() {
                    return Err(invalid(format!(
                        "Cannot access the Wayland clipboard: {error}. Enable your compositor's data-control protocol or import the image from a file."
                    )));
                }
            }
            Err(WaylandError::Transfer(error)) => return Err(error),
            Ok(image) => return Ok(image),
        }
    }
    x11::read_image()
}

enum WaylandError {
    Unavailable(paste::Error),
    Transfer(crate::Error),
}

fn read_wayland() -> std::result::Result<Option<Vec<u8>>, WaylandError> {
    let types = match paste::get_mime_types_ordered(ClipboardType::Regular, Seat::Unspecified) {
        Ok(types) => types,
        Err(paste::Error::ClipboardEmpty | paste::Error::NoMimeType) => return Ok(None),
        Err(error) => return Err(WaylandError::Unavailable(error)),
    };
    let Some(mime) = types.iter().find(|mime| supported(mime)) else {
        return Ok(None);
    };
    let (pipe, _) = paste::get_contents(
        ClipboardType::Regular,
        Seat::Unspecified,
        MimeType::Specific(mime),
    )
    .map_err(|error| {
        WaylandError::Transfer(invalid(format!(
            "Could not receive the clipboard image: {error}. Copy the image again and retry. The document is unchanged."
        )))
    })?;
    read_pipe(pipe, MAX_BYTES, TIMEOUT)
        .map(Some)
        .map_err(WaylandError::Transfer)
}

fn read_pipe(mut pipe: impl Read + AsFd, limit: usize, timeout: Duration) -> Result<Vec<u8>> {
    use rustix::fs::{OFlags, fcntl_getfl, fcntl_setfl};
    let flags = fcntl_getfl(&pipe).map_err(std::io::Error::from)?;
    fcntl_setfl(&pipe, flags | OFlags::NONBLOCK).map_err(std::io::Error::from)?;
    let deadline = Instant::now() + timeout;
    let mut bytes = Vec::new();
    let mut chunk = [0; 64 * 1024];
    loop {
        check_deadline(deadline)?;
        match pipe.read(&mut chunk) {
            Ok(0) => return Ok(bytes),
            Ok(count) => append(&mut bytes, &chunk[..count], limit)?,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                wait_readable(&pipe, deadline)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error.into()),
        }
    }
}

fn wait_readable(fd: impl AsFd, deadline: Instant) -> Result<()> {
    use rustix::event::{PollFd, PollFlags, Timespec, poll};
    check_deadline(deadline)?;
    let remaining = deadline.saturating_duration_since(Instant::now());
    let timeout = Timespec {
        tv_sec: remaining.as_secs() as i64,
        tv_nsec: remaining.subsec_nanos() as i64,
    };
    match poll(&mut [PollFd::new(&fd, PollFlags::IN)], Some(&timeout)) {
        Ok(_) | Err(rustix::io::Errno::INTR) => Ok(()),
        Err(error) => Err(std::io::Error::from(error).into()),
    }
}

fn check_deadline(deadline: Instant) -> Result<()> {
    if Instant::now() >= deadline {
        return Err(invalid(
            "The clipboard image transfer timed out. Copy the image again or import it from a file. The document is unchanged.",
        ));
    }
    Ok(())
}

fn append(bytes: &mut Vec<u8>, chunk: &[u8], limit: usize) -> Result<()> {
    if chunk.len() > limit.saturating_sub(bytes.len()) {
        return Err(invalid(
            "The encoded clipboard image exceeds the transfer size limit. Import a smaller image from a file. The document is unchanged.",
        ));
    }
    bytes.extend_from_slice(chunk);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Write, os::unix::net::UnixStream};

    #[test]
    fn transfers_preserve_bytes_and_reject_oversized_or_stalled_senders() {
        let (mut writer, reader) = UnixStream::pair().unwrap();
        let bytes = b"original encoded image with ICC and EXIF";
        writer.write_all(bytes).unwrap();
        drop(writer);
        assert_eq!(read_pipe(reader, bytes.len(), TIMEOUT).unwrap(), bytes);

        let (mut writer, reader) = UnixStream::pair().unwrap();
        writer.write_all(b"too large").unwrap();
        assert!(
            read_pipe(reader, 3, TIMEOUT)
                .unwrap_err()
                .to_string()
                .contains("size limit")
        );

        let (_writer, reader) = UnixStream::pair().unwrap();
        assert!(
            read_pipe(reader, 100, Duration::from_millis(10))
                .unwrap_err()
                .to_string()
                .contains("timed out")
        );
    }
}
