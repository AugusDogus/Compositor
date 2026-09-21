//! winit's Wayland focus_window is a no-op. Activate an existing window using
//! the currently focused surface as the source of a compositor-issued token.
use sctk::reexports::{
    calloop::EventLoop,
    calloop_wayland_source::WaylandSource,
    client::{
        Connection, Dispatch, Proxy, QueueHandle, WEnum,
        globals::{GlobalList, GlobalListContents, registry_queue_init},
        protocol::{
            wl_keyboard::{self, WlKeyboard},
            wl_registry::WlRegistry,
            wl_seat::{self, WlSeat},
            wl_surface::WlSurface,
        },
    },
    protocols::xdg::activation::v1::client::{
        xdg_activation_token_v1::{self, XdgActivationTokenV1},
        xdg_activation_v1::XdgActivationV1,
    },
};
use std::{
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};
use wayland_backend::client::{Backend, ObjectId};
use winit::{
    event_loop::{ActiveEventLoop, OwnedDisplayHandle},
    raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle},
    window::Window,
};

type Request = (Arc<Window>, Arc<Window>);
pub(super) struct Requests(mpsc::SyncSender<Request>);

impl Requests {
    fn new(display: OwnedDisplayHandle) -> std::io::Result<Self> {
        let (send, receive) = mpsc::sync_channel(8);
        std::thread::Builder::new()
            .name("wayland-activation".into())
            .spawn(move || {
                if let Err(error) = run(display, receive) {
                    tracing::warn!(%error, "Wayland window activation worker stopped");
                }
            })?;
        Ok(Self(send))
    }
}

/// Returns true for Wayland, whose native focus event must remain authoritative.
pub(super) fn request(
    worker: &mut Option<Requests>,
    event_loop: &ActiveEventLoop,
    target: Arc<Window>,
    source: Option<Arc<Window>>,
) -> bool {
    if !matches!(
        target.display_handle().map(|h| h.as_raw()),
        Ok(RawDisplayHandle::Wayland(_))
    ) {
        return false;
    }
    let Some(source) = source else { return true };
    if source.id() == target.id() {
        return true;
    }
    if worker.is_none() {
        match Requests::new(event_loop.owned_display_handle()) {
            Ok(requests) => *worker = Some(requests),
            Err(error) => {
                tracing::warn!(%error, "could not start Wayland window activation");
                return true;
            }
        }
    }
    if let Some(requests) = worker {
        match requests.0.try_send((source, target)) {
            Ok(()) => {}
            Err(mpsc::TrySendError::Full(_)) => tracing::warn!("Wayland activation queue is full"),
            Err(mpsc::TrySendError::Disconnected(_)) => {
                tracing::warn!(
                    "Wayland activation worker disconnected; the next focus request will restart it"
                );
                *worker = None;
            }
        }
    }
    true
}

fn surface(
    window: &Window,
    connection: &Connection,
) -> Result<WlSurface, Box<dyn std::error::Error>> {
    let RawWindowHandle::Wayland(raw) = window.window_handle()?.as_raw() else {
        return Err("activation requires a Wayland window".into());
    };
    // SAFETY: the caller retains the Window throughout the request. The proxy
    // borrows its live wl_surface and is never destroyed by this worker.
    let id = unsafe { ObjectId::from_ptr(WlSurface::interface(), raw.surface.as_ptr().cast()) }?;
    Ok(WlSurface::from_id(connection, id)?)
}

fn run(
    display: OwnedDisplayHandle,
    receive: mpsc::Receiver<Request>,
) -> Result<(), Box<dyn std::error::Error>> {
    let RawDisplayHandle::Wayland(raw) = display.display_handle()?.as_raw() else {
        return Err("activation requires a Wayland display".into());
    };
    // SAFETY: display owns the foreign wl_display until the connection and
    // queue are dropped. Each request separately retains its native windows.
    let backend = unsafe { Backend::from_foreign_display(raw.display.as_ptr().cast()) };
    let connection = Connection::from_backend(backend);
    let (globals, queue) = registry_queue_init(&connection)?;
    let handle = queue.handle();
    let mut events = EventLoop::<Activation>::try_new()?;
    WaylandSource::new(connection.clone(), queue).insert(events.handle())?;
    // Reuse one registry, including on compositors predating wl_fixes (which
    // cannot release registry objects without closing the shared connection).
    for (source, target) in receive {
        if !source.has_focus() {
            continue;
        }
        if let Err(error) = activate(
            &source,
            &target,
            &connection,
            &globals,
            &handle,
            &mut events,
        ) {
            tracing::warn!(%error, "could not activate Wayland window");
        }
    }
    globals.destroy();
    connection.flush()?;
    Ok(())
}

fn activate(
    source: &Window,
    target: &Window,
    connection: &Connection,
    globals: &GlobalList,
    handle: &QueueHandle<Activation>,
    events: &mut EventLoop<'_, Activation>,
) -> Result<(), Box<dyn std::error::Error>> {
    let source_surface = surface(source, connection)?;
    let target_surface = surface(target, connection)?;
    let activation: XdgActivationV1 = globals.bind(handle, 1..=1, ())?;
    let mut state = Activation {
        activation,
        source: source_surface,
        target: target_surface,
        token: None,
        seats: Vec::new(),
        keyboards: Vec::new(),
        connection: connection.clone(),
        done: false,
    };
    globals.contents().with_list(|list| {
        for global in list.iter().filter(|global| global.interface == "wl_seat") {
            state.seats.push(globals.registry().bind(
                global.name,
                global.version.min(7),
                handle,
                (),
            ));
        }
    });
    let deadline = Instant::now() + Duration::from_secs(2);
    while !state.done {
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            return Err("Wayland activation token timed out".into());
        };
        events.dispatch(Some(remaining), &mut state)?;
    }
    connection.flush()?;
    Ok(())
}

struct Activation {
    activation: XdgActivationV1,
    source: WlSurface,
    target: WlSurface,
    token: Option<XdgActivationTokenV1>,
    seats: Vec<WlSeat>,
    keyboards: Vec<WlKeyboard>,
    connection: Connection,
    done: bool,
}

// A foreign connection outlives this worker. Explicitly release the protocol
// objects it created on success, timeout and dispatch failure.
impl Drop for Activation {
    fn drop(&mut self) {
        if let Some(token) = &self.token
            && token.is_alive()
        {
            token.destroy();
        }
        for keyboard in &self.keyboards {
            if keyboard.version() >= 3 {
                keyboard.release();
            }
        }
        for seat in &self.seats {
            if seat.version() >= 5 {
                seat.release();
            }
        }
        self.activation.destroy();
        if let Err(error) = self.connection.flush() {
            tracing::warn!(%error, "could not flush Wayland activation cleanup");
        }
    }
}

impl Dispatch<WlSeat, ()> for Activation {
    fn event(
        state: &mut Self,
        seat: &WlSeat,
        event: wl_seat::Event,
        _: &(),
        _: &Connection,
        queue: &QueueHandle<Self>,
    ) {
        if let wl_seat::Event::Capabilities {
            capabilities: WEnum::Value(capabilities),
        } = event
            && capabilities.contains(wl_seat::Capability::Keyboard)
            && state.seats.contains(seat)
            && !state
                .keyboards
                .iter()
                .any(|keyboard| keyboard.data::<WlSeat>() == Some(seat))
        {
            state.keyboards.push(seat.get_keyboard(queue, seat.clone()));
        }
    }
}

impl Dispatch<WlKeyboard, WlSeat> for Activation {
    fn event(
        state: &mut Self,
        _: &WlKeyboard,
        event: wl_keyboard::Event,
        seat: &WlSeat,
        _: &Connection,
        queue: &QueueHandle<Self>,
    ) {
        if let wl_keyboard::Event::Enter {
            serial, surface, ..
        } = event
            && surface == state.source
            && state.seats.contains(seat)
            && state.token.is_none()
        {
            let token = state.activation.get_activation_token(queue, ());
            token.set_surface(&state.source);
            token.set_serial(serial, seat);
            token.commit();
            state.token = Some(token);
        }
    }
}

impl Dispatch<WlRegistry, GlobalListContents> for Activation {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: <WlRegistry as Proxy>::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<XdgActivationV1, ()> for Activation {
    fn event(
        _: &mut Self,
        _: &XdgActivationV1,
        _: <XdgActivationV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<XdgActivationTokenV1, ()> for Activation {
    fn event(
        state: &mut Self,
        token: &XdgActivationTokenV1,
        event: xdg_activation_token_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_activation_token_v1::Event::Done { token: value } = event
            && state.token.as_ref() == Some(token)
        {
            state.activation.activate(value, &state.target);
            token.destroy();
            state.done = true;
        }
    }
}
