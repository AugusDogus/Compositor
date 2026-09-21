use super::*;
use sctk::reexports::client::{
    Connection, Dispatch, Proxy, QueueHandle,
    globals::GlobalList,
    protocol::{
        wl_data_device::WlDataDevice,
        wl_data_device_manager::DndAction,
        wl_data_source::WlDataSource,
        wl_keyboard::{self, WlKeyboard},
        wl_pointer::WlPointer,
        wl_seat::WlSeat,
        wl_surface::WlSurface,
    },
};
use sctk::{
    data_device_manager::{
        DataDeviceManagerState, WritePipe,
        data_device::{DataDevice, DataDeviceHandler},
        data_offer::{DataOfferHandler, DragOffer},
        data_source::{CopyPasteSource, DataSourceHandler},
    },
    delegate_data_device, delegate_pointer, delegate_registry, delegate_seat,
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        pointer::{PointerData, PointerEvent, PointerEventKind, PointerHandler},
    },
};
use std::sync::atomic::AtomicUsize;
use wayland_backend::client::ObjectId;

pub(super) struct State {
    pub exit: bool,
    reads: Vec<PendingRead>,
    writes: Vec<(Option<Payload>, mpsc::Sender<Result<()>>)>,
    registry: RegistryState,
    seat_state: SeatState,
    manager: DataDeviceManagerState,
    queue: QueueHandle<Self>,
    seats: Vec<Seat>,
    sources: Vec<(CopyPasteSource, Payload)>,
    transfers: Arc<AtomicUsize>,
}

struct PendingRead {
    kind: Kind,
    reply: mpsc::Sender<Result<Option<Payload>>>,
    deadline: std::time::Instant,
}

struct Seat {
    id: ObjectId,
    keyboard: Option<WlKeyboard>,
    pointer: Option<WlPointer>,
    device: Option<DataDevice>,
    // Only focused seats carry a serial usable for setting their clipboard.
    focused_serial: Option<u32>,
}

impl Seat {
    fn new(seat: &WlSeat) -> Self {
        Self {
            id: seat.id(),
            keyboard: None,
            pointer: None,
            device: None,
            focused_serial: None,
        }
    }
}

impl State {
    pub fn new(globals: &GlobalList, queue: &QueueHandle<Self>) -> Result<Self> {
        let seat_state = SeatState::new(globals, queue);
        let seats = seat_state.seats().map(|seat| Seat::new(&seat)).collect();
        Ok(Self {
            exit: false,
            reads: Vec::new(),
            writes: Vec::new(),
            registry: RegistryState::new(globals),
            seat_state,
            seats,
            manager: DataDeviceManagerState::bind(globals, queue).map_err(|e| {
                invalid(format!(
                    "The compositor does not provide wl_data_device_manager: {e}"
                ))
            })?,
            queue: queue.clone(),
            sources: Vec::new(),
            transfers: Arc::new(AtomicUsize::new(0)),
        })
    }

    pub fn queue_read(&mut self, kind: Kind, reply: mpsc::Sender<Result<Option<Payload>>>) {
        if self.reads.len() >= 4 {
            let _ = reply.send(Err(invalid(
                "Clipboard requests are still pending. Retry in a moment.",
            )));
            return;
        }
        self.reads.push(PendingRead {
            kind,
            reply,
            deadline: std::time::Instant::now() + Duration::from_secs(1),
        });
    }

    pub fn queue_write(&mut self, payload: Option<Payload>, reply: mpsc::Sender<Result<()>>) {
        if self.writes.len() >= 4 {
            let _ = reply.send(Err(invalid(
                "Clipboard writes are still pending. Copy again in a moment.",
            )));
            return;
        }
        self.writes.push((payload, reply));
    }

    pub fn finish_requests(&mut self) {
        for (payload, reply) in std::mem::take(&mut self.writes) {
            let _ = reply.send(self.write(payload));
        }
        // Process after dispatching the event batch: selection offers follow
        // keyboard enter, and initial presentation can precede keyboard focus.
        for pending in std::mem::take(&mut self.reads) {
            if self.focused_seat().is_err() && std::time::Instant::now() < pending.deadline {
                self.reads.push(pending);
            } else if let Err(error) = self.read(pending.kind, pending.reply.clone()) {
                let _ = pending.reply.send(Err(error));
            }
        }
    }

    fn focused_seat(&self) -> Result<(&DataDevice, u32)> {
        self.seats.iter().find_map(|seat| Some((seat.device.as_ref()?, seat.focused_serial?)))
            .ok_or_else(|| invalid("The editor does not have keyboard focus. Focus its window and retry the clipboard operation."))
    }

    pub fn read(&mut self, kind: Kind, reply: mpsc::Sender<Result<Option<Payload>>>) -> Result<()> {
        let (device, _) = self.focused_seat()?;
        let Some(offer) = device.data().selection_offer() else {
            let _ = reply.send(Ok(None));
            return Ok(());
        };
        let mime = offer.with_mime_types(|types| {
            let accepted: &[&str] = match kind {
                Kind::Image => &["image/png", "image/tiff", "image/jpeg", "image/webp"],
                Kind::Text => &["text/plain;charset=utf-8", "UTF8_STRING", "text/plain"],
            };
            accepted
                .iter()
                .find(|candidate| types.iter().any(|mime| mime == **candidate))
                .map(|mime| (*mime).to_string())
        });
        let Some(mime) = mime else {
            let _ = reply.send(Ok(None));
            return Ok(());
        };
        let permit = transfer::Permit::acquire(&self.transfers)?;
        let pipe = offer
            .receive(mime.clone())
            .map_err(|e| invalid(format!("Could not receive the Wayland clipboard: {e}")))?;
        let limit = match kind {
            Kind::Image => super::super::MAX_BYTES,
            Kind::Text => quickgui::MAX_CLIPBOARD_TEXT_BYTES,
        };
        std::thread::Builder::new()
            .name("clipboard-read".into())
            .spawn(move || {
                let _permit = permit;
                let result =
                    super::super::read_pipe(pipe, limit, super::super::TIMEOUT).map(|bytes| {
                        Some(Payload {
                            mime,
                            bytes: Arc::new(bytes),
                        })
                    });
                let _ = reply.send(result);
            })?;
        Ok(())
    }

    pub fn write(&mut self, payload: Option<Payload>) -> Result<()> {
        if self.sources.len() >= 8 {
            return Err(invalid(
                "Previous clipboard offers are still being released. Copy again in a moment.",
            ));
        }
        let (device, serial) = self.focused_seat()?;
        match payload {
            None => device.inner().set_selection(None, serial),
            Some(payload) => {
                let types = if payload.mime.starts_with("text/plain") {
                    vec!["text/plain;charset=utf-8", "UTF8_STRING", "text/plain"]
                } else {
                    vec![payload.mime.as_str()]
                };
                let source = self.manager.create_copy_paste_source(&self.queue, types);
                source.set_selection(device, serial);
                self.sources.push((source, payload));
            }
        }
        Ok(())
    }
}

impl SeatHandler for State {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, seat: WlSeat) {
        if !self.seats.iter().any(|s| s.id == seat.id()) {
            self.seats.push(Seat::new(&seat));
        }
    }
    fn new_capability(
        &mut self,
        _: &Connection,
        queue: &QueueHandle<Self>,
        seat: WlSeat,
        capability: Capability,
    ) {
        let Some(state) = self.seats.iter_mut().find(|s| s.id == seat.id()) else {
            return;
        };
        match capability {
            Capability::Keyboard => {
                state.keyboard = Some(seat.get_keyboard(queue, seat.id()));
                state.device = Some(self.manager.get_data_device(queue, &seat));
            }
            Capability::Pointer => state.pointer = self.seat_state.get_pointer(queue, &seat).ok(),
            _ => {}
        }
    }
    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        seat: WlSeat,
        capability: Capability,
    ) {
        let Some(state) = self.seats.iter_mut().find(|s| s.id == seat.id()) else {
            return;
        };
        match capability {
            Capability::Keyboard => {
                state.device = None;
                state.focused_serial = None;
                if let Some(keyboard) = state.keyboard.take()
                    && keyboard.version() >= 3
                {
                    keyboard.release();
                }
            }
            Capability::Pointer => {
                if let Some(pointer) = state.pointer.take()
                    && pointer.version() >= 3
                {
                    pointer.release();
                }
            }
            _ => {}
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, seat: WlSeat) {
        self.seats.retain(|s| s.id != seat.id());
    }
}

impl Dispatch<WlKeyboard, ObjectId> for State {
    fn event(
        state: &mut Self,
        _: &WlKeyboard,
        event: wl_keyboard::Event,
        seat: &ObjectId,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(seat) = state.seats.iter_mut().find(|s| s.id == *seat) else {
            return;
        };
        match event {
            wl_keyboard::Event::Enter { serial, .. } => seat.focused_serial = Some(serial),
            wl_keyboard::Event::Leave { .. } => seat.focused_serial = None,
            wl_keyboard::Event::Key { serial, .. }
            | wl_keyboard::Event::Modifiers { serial, .. }
                if seat.focused_serial.is_some() =>
            {
                seat.focused_serial = Some(serial)
            }
            _ => {}
        }
    }
}

impl PointerHandler for State {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        pointer: &WlPointer,
        events: &[PointerEvent],
    ) {
        let Some(data) = pointer.data::<PointerData>() else {
            return;
        };
        let Some(seat) = self.seats.iter_mut().find(|s| s.id == data.seat().id()) else {
            return;
        };
        for event in events {
            if let PointerEventKind::Press { serial, .. } | PointerEventKind::Release { serial, .. } =
                event.kind
                && seat.focused_serial.is_some()
            {
                seat.focused_serial = Some(serial);
            }
        }
    }
}

impl DataDeviceHandler for State {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlDataDevice,
        _: f64,
        _: f64,
        _: &WlSurface,
    ) {
    }
    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {}
    fn motion(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice, _: f64, _: f64) {}
    fn drop_performed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {}
    fn selection(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {}
}

impl DataSourceHandler for State {
    fn send_request(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        source: &WlDataSource,
        mime: String,
        pipe: WritePipe,
    ) {
        let Some((_, payload)) = self.sources.iter().find(|(s, _)| s.inner() == source) else {
            return;
        };
        if mime != payload.mime
            && !(payload.mime.starts_with("text/plain")
                && matches!(mime.as_str(), "UTF8_STRING" | "text/plain"))
        {
            return;
        }
        let result = transfer::Permit::acquire(&self.transfers).and_then(|permit| {
            let bytes = payload.bytes.clone();
            std::thread::Builder::new()
                .name("clipboard-write".into())
                .spawn(move || {
                    let _permit = permit;
                    if let Err(error) = transfer::write(pipe, &bytes) {
                        eprintln!("Clipboard receiver did not finish its transfer: {error}");
                    }
                })
                .map(|_| ())
                .map_err(crate::Error::from)
        });
        if let Err(error) = result {
            eprintln!("Could not serve clipboard data: {error}");
        }
    }
    fn cancelled(&mut self, _: &Connection, _: &QueueHandle<Self>, source: &WlDataSource) {
        self.sources.retain(|(s, _)| s.inner() != source);
    }
    fn accept_mime(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlDataSource,
        _: Option<String>,
    ) {
    }
    fn dnd_dropped(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource) {}
    fn dnd_finished(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource) {}
    fn action(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource, _: DndAction) {}
}

impl DataOfferHandler for State {
    fn source_actions(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &mut DragOffer,
        _: DndAction,
    ) {
    }
    fn selected_action(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &mut DragOffer,
        _: DndAction,
    ) {
    }
}

impl ProvidesRegistryState for State {
    registry_handlers![SeatState];
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }
}

delegate_seat!(State);
delegate_pointer!(State);
delegate_data_device!(State);
delegate_registry!(State);
