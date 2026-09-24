use super::{MAX_BYTES, TIMEOUT, append, check_deadline, supported, wait_readable};
use crate::{Result, invalid};
use std::time::Instant;
use x11rb::{
    COPY_DEPTH_FROM_PARENT, CURRENT_TIME, NONE,
    connection::Connection,
    protocol::{
        Event,
        xproto::{
            Atom, AtomEnum, ConnectionExt, CreateWindowAux, EventMask, GetPropertyReply, Property,
            Window, WindowClass,
        },
    },
    rust_connection::RustConnection,
};

fn error(error: impl std::fmt::Display) -> crate::Error {
    invalid(format!(
        "Could not read the X11 clipboard: {error}. Copy the image again or import it from a file. The document is unchanged."
    ))
}

struct Reader {
    connection: RustConnection,
    window: Window,
    clipboard: Atom,
    property: Atom,
    incr: Atom,
}

pub(super) fn read_image() -> Result<Option<Vec<u8>>> {
    let reader = Reader::new()?;
    let targets = reader.atom(b"TARGETS")?;
    let Some(reply) = reader.request(targets, 64 * 1024)? else {
        return Ok(None);
    };
    if reply.format != 32 || reply.type_ != u32::from(AtomEnum::ATOM) {
        return Err(error("the image owner returned an invalid TARGETS list"));
    }
    // TARGETS and value32 use the connection's native byte order.
    for target in reply.value.chunks_exact(4) {
        let target = u32::from_ne_bytes([target[0], target[1], target[2], target[3]]);
        let name = reader
            .connection
            .get_atom_name(target)
            .map_err(error)?
            .reply()
            .map_err(error)?;
        if std::str::from_utf8(&name.name).is_ok_and(supported) {
            return reader.request(target, MAX_BYTES).and_then(|reply| {
                let reply = reply.ok_or_else(|| {
                    error("the image owner stopped offering the requested format")
                })?;
                if reply.type_ != target || reply.format != 8 {
                    return Err(error("the image owner returned an unexpected image format"));
                }
                Ok(Some(reply.value))
            });
        }
    }
    Ok(None)
}

impl Reader {
    fn new() -> Result<Self> {
        let (connection, screen) = x11rb::connect(None).map_err(error)?;
        let root = connection
            .setup()
            .roots
            .get(screen)
            .ok_or_else(|| error("the display has no screen"))?;
        let window = connection.generate_id().map_err(error)?;
        connection
            .create_window(
                COPY_DEPTH_FROM_PARENT,
                window,
                root.root,
                0,
                0,
                1,
                1,
                0,
                WindowClass::INPUT_OUTPUT,
                root.root_visual,
                &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
            )
            .map_err(error)?
            .check()
            .map_err(error)?;
        let atom = |name: &[u8]| -> Result<Atom> {
            Ok(connection
                .intern_atom(false, name)
                .map_err(error)?
                .reply()
                .map_err(error)?
                .atom)
        };
        let clipboard = atom(b"CLIPBOARD")?;
        let property = atom(b"COMPOSITOR_CLIPBOARD_IMAGE")?;
        let incr = atom(b"INCR")?;
        Ok(Self {
            connection,
            window,
            clipboard,
            property,
            incr,
        })
    }

    fn atom(&self, name: &[u8]) -> Result<Atom> {
        Ok(self
            .connection
            .intern_atom(false, name)
            .map_err(error)?
            .reply()
            .map_err(error)?
            .atom)
    }

    fn property(&self, limit: usize) -> Result<GetPropertyReply> {
        let reply = self
            .connection
            .get_property(
                true,
                self.window,
                self.property,
                AtomEnum::ANY,
                0,
                limit.div_ceil(4) as u32,
            )
            .map_err(error)?
            .reply()
            .map_err(error)?;
        if reply.bytes_after != 0 || reply.value.len() > limit {
            return Err(error("the encoded image exceeds the transfer size limit"));
        }
        Ok(reply)
    }

    fn request(&self, target: Atom, limit: usize) -> Result<Option<GetPropertyReply>> {
        self.connection
            .convert_selection(
                self.window,
                self.clipboard,
                target,
                self.property,
                CURRENT_TIME,
            )
            .map_err(error)?
            .check()
            .map_err(error)?;
        self.connection.flush().map_err(error)?;
        let deadline = Instant::now() + TIMEOUT;
        let mut incremental: Option<GetPropertyReply> = None;
        loop {
            check_deadline(deadline)?;
            let Some(event) = self.connection.poll_for_event().map_err(error)? else {
                wait_readable(self.connection.stream(), deadline)?;
                continue;
            };
            match event {
                Event::SelectionNotify(event)
                    if event.requestor == self.window
                        && event.selection == self.clipboard
                        && event.target == target =>
                {
                    if event.property == NONE {
                        return Ok(None);
                    }
                    if event.property != self.property {
                        return Err(error("the image owner returned an unexpected property"));
                    }
                    let reply = self.property(limit)?;
                    if reply.type_ != self.incr {
                        return Ok(Some(reply));
                    }
                    // The INCR size is only a lower bound. Do not reserve it or
                    // trust it instead of checking each received chunk.
                    self.connection.flush().map_err(error)?;
                    incremental = Some(GetPropertyReply {
                        value: Vec::new(),
                        type_: 0,
                        format: 0,
                        ..reply
                    });
                }
                Event::PropertyNotify(event)
                    if incremental.is_some()
                        && event.window == self.window
                        && event.atom == self.property
                        && event.state == Property::NEW_VALUE =>
                {
                    let Some(accumulated) = &mut incremental else {
                        continue;
                    };
                    let reply = self.property(limit.saturating_sub(accumulated.value.len()))?;
                    if accumulated.type_ == 0 {
                        accumulated.type_ = reply.type_;
                        accumulated.format = reply.format;
                    } else if accumulated.type_ != reply.type_ || accumulated.format != reply.format
                    {
                        return Err(error("the image format changed during transfer"));
                    }
                    if reply.value.is_empty() {
                        return Ok(incremental);
                    }
                    append(&mut accumulated.value, &reply.value, limit)?;
                    self.connection.flush().map_err(error)?;
                }
                _ => {}
            }
        }
    }
}

pub(super) fn owner() -> Result<u32> {
    let (connection, _) = x11rb::connect(None).map_err(error)?;
    let clipboard = connection
        .intern_atom(false, b"CLIPBOARD")
        .map_err(error)?
        .reply()
        .map_err(error)?
        .atom;
    Ok(connection
        .get_selection_owner(clipboard)
        .map_err(error)?
        .reply()
        .map_err(error)?
        .owner)
}
