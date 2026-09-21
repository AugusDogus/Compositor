use super::*;
use raw_window_handle::{HasDisplayHandle, RawDisplayHandle};
use sctk::reexports::{
    calloop::{EventLoop, channel},
    calloop_wayland_source::WaylandSource,
    client::{Connection, globals::registry_queue_init},
};
use wayland_backend::client::Backend;

pub(super) fn run(
    display: OwnedDisplayHandle,
    commands: channel::Channel<Command>,
    ready: &mpsc::Sender<Result<()>>,
) -> Result<()> {
    let RawDisplayHandle::Wayland(raw) = display
        .display_handle()
        .map_err(|e| invalid(e.to_string()))?
        .as_raw()
    else {
        return Err(invalid("The clipboard requires a Wayland display."));
    };
    // SAFETY: display owns a reference to this wl_display. It outlives the
    // connection and event queue declared below, including on early returns.
    let backend = unsafe { Backend::from_foreign_display(raw.display.as_ptr().cast()) };
    let connection = Connection::from_backend(backend);
    let (globals, event_queue) = registry_queue_init(&connection).map_err(|e| {
        invalid(format!(
            "Could not enumerate Wayland clipboard services: {e}"
        ))
    })?;
    let mut event_loop = EventLoop::<state::State>::try_new()
        .map_err(|e| invalid(format!("Could not create the clipboard event loop: {e}")))?;
    let mut state = state::State::new(&globals, &event_queue.handle())?;
    event_loop
        .handle()
        .insert_source(commands, |event, _, state| match event {
            channel::Event::Msg(Command::Read(kind, reply)) => {
                state.queue_read(kind, reply);
            }
            channel::Event::Msg(Command::Write(payload, reply)) => {
                state.queue_write(payload, reply);
            }
            channel::Event::Msg(Command::Exit) | channel::Event::Closed => state.exit = true,
        })
        .map_err(|e| invalid(format!("Could not start clipboard commands: {e}")))?;
    WaylandSource::new(connection, event_queue)
        .insert(event_loop.handle())
        .map_err(|e| invalid(format!("Could not listen for clipboard changes: {e}")))?;
    let _ = ready.send(Ok(()));
    while !state.exit {
        event_loop
            .dispatch(Some(Duration::from_millis(100)), &mut state)
            .map_err(|e| invalid(format!("Wayland clipboard dispatch failed: {e}")))?;
        state.finish_requests();
    }
    Ok(())
}
