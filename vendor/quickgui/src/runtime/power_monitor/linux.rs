use std::{
    sync::mpsc,
    thread::{self, JoinHandle},
};

use winit::event_loop::EventLoopProxy;
use zbus::{
    MatchRule,
    blocking::{Connection, MessageIterator, Proxy},
    message::Type,
    zvariant::OwnedObjectPath,
};

use super::{PowerEvent, RuntimeEvent};

pub(crate) struct LinuxPowerMonitor {
    sleep_connection: Option<Connection>,
    sleep_thread: Option<JoinHandle<()>>,
    session_connection: Option<Connection>,
    session_thread: Option<JoinHandle<()>>,
}

impl LinuxPowerMonitor {
    pub(crate) fn start(proxy: EventLoopProxy<RuntimeEvent>) -> Result<Self, String> {
        let connection = Connection::system().map_err(|error| error.to_string())?;
        let thread_connection = connection.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        let sleep_proxy = proxy.clone();
        let thread = thread::Builder::new()
            .name("quickgui-power-monitor".to_owned())
            .spawn(move || monitor_logind(thread_connection, sleep_proxy, sender))
            .map_err(|error| error.to_string())?;
        let (sleep_connection, sleep_thread) = match receiver.recv() {
            Ok(Ok(())) => (connection, thread),
            Ok(Err(error)) => {
                let _ = connection.close();
                let _ = thread.join();
                return Err(error);
            }
            Err(error) => {
                let _ = connection.close();
                let _ = thread.join();
                return Err(error.to_string());
            }
        };

        let (session_connection, session_thread) = match start_session_monitor(proxy) {
            Ok((connection, thread)) => (Some(connection), Some(thread)),
            Err(error) => {
                tracing::warn!(%error, "Linux session lock monitoring is unavailable");
                (None, None)
            }
        };
        Ok(Self {
            sleep_connection: Some(sleep_connection),
            sleep_thread: Some(sleep_thread),
            session_connection,
            session_thread,
        })
    }
}

fn start_session_monitor(
    proxy: EventLoopProxy<RuntimeEvent>,
) -> Result<(Connection, JoinHandle<()>), String> {
    let connection = Connection::system().map_err(|error| error.to_string())?;
    let thread_connection = connection.clone();
    let (sender, receiver) = mpsc::sync_channel(1);
    let thread = thread::Builder::new()
        .name("quickgui-session-monitor".to_owned())
        .spawn(move || monitor_logind_session(thread_connection, proxy, sender))
        .map_err(|error| error.to_string())?;
    match receiver.recv() {
        Ok(Ok(())) => Ok((connection, thread)),
        Ok(Err(error)) => {
            let _ = connection.close();
            let _ = thread.join();
            Err(error)
        }
        Err(error) => {
            let _ = connection.close();
            let _ = thread.join();
            Err(error.to_string())
        }
    }
}

impl Drop for LinuxPowerMonitor {
    fn drop(&mut self) {
        if let Some(connection) = self.sleep_connection.take() {
            let _ = connection.close();
        }
        if let Some(connection) = self.session_connection.take() {
            let _ = connection.close();
        }
        if let Some(thread) = self.sleep_thread.take() {
            let _ = thread.join();
        }
        if let Some(thread) = self.session_thread.take() {
            let _ = thread.join();
        }
    }
}

fn monitor_logind(
    connection: Connection,
    event_proxy: EventLoopProxy<RuntimeEvent>,
    ready: mpsc::SyncSender<Result<(), String>>,
) {
    let rule = match MatchRule::builder()
        .msg_type(Type::Signal)
        .sender("org.freedesktop.login1")
        .and_then(|builder| builder.interface("org.freedesktop.login1.Manager"))
        .and_then(|builder| builder.path("/org/freedesktop/login1"))
    {
        Ok(builder) => builder.build(),
        Err(error) => {
            let _ = ready.send(Err(error.to_string()));
            return;
        }
    };
    let mut signals = match MessageIterator::for_match_rule(rule, &connection, Some(16)) {
        Ok(signals) => signals,
        Err(error) => {
            let _ = ready.send(Err(error.to_string()));
            return;
        }
    };
    if ready.send(Ok(())).is_err() {
        return;
    }
    let mut suspended = false;
    for message in &mut signals {
        let Ok(message) = message else {
            break;
        };
        let header = message.header();
        let member = header.member().map(|member| member.as_str());
        let Ok(value) = message.body().deserialize::<bool>() else {
            continue;
        };
        let event = match member {
            Some("PrepareForSleep") if value && !suspended => {
                suspended = true;
                Some(PowerEvent::Suspend)
            }
            Some("PrepareForSleep") if !value && suspended => {
                suspended = false;
                Some(PowerEvent::Resume)
            }
            Some("PrepareForShutdown") if value => Some(PowerEvent::ShutdownRequested),
            _ => None,
        };
        if let Some(event) = event
            && event_proxy.send_event(RuntimeEvent::Power(event)).is_err()
        {
            break;
        }
    }
}

fn monitor_logind_session(
    connection: Connection,
    event_proxy: EventLoopProxy<RuntimeEvent>,
    ready: mpsc::SyncSender<Result<(), String>>,
) {
    let manager = match Proxy::new(
        &connection,
        "org.freedesktop.login1",
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
    ) {
        Ok(manager) => manager,
        Err(error) => {
            let _ = ready.send(Err(error.to_string()));
            return;
        }
    };
    let session_path: OwnedObjectPath =
        match manager.call("GetSessionByPID", &(std::process::id(),)) {
            Ok(path) => path,
            Err(error) => {
                let _ = ready.send(Err(error.to_string()));
                return;
            }
        };
    let rule = match MatchRule::builder()
        .msg_type(Type::Signal)
        .sender("org.freedesktop.login1")
        .and_then(|builder| builder.interface("org.freedesktop.login1.Session"))
        .and_then(|builder| builder.path(session_path))
    {
        Ok(builder) => builder.build(),
        Err(error) => {
            let _ = ready.send(Err(error.to_string()));
            return;
        }
    };
    let mut signals = match MessageIterator::for_match_rule(rule, &connection, Some(16)) {
        Ok(signals) => signals,
        Err(error) => {
            let _ = ready.send(Err(error.to_string()));
            return;
        }
    };
    if ready.send(Ok(())).is_err() {
        return;
    }
    let mut locked = false;
    for message in &mut signals {
        let Ok(message) = message else {
            break;
        };
        let event = match message.header().member().map(|member| member.as_str()) {
            Some("Lock") if !locked => {
                locked = true;
                Some(PowerEvent::LockScreen)
            }
            Some("Unlock") if locked => {
                locked = false;
                Some(PowerEvent::UnlockScreen)
            }
            _ => None,
        };
        if let Some(event) = event
            && event_proxy.send_event(RuntimeEvent::Power(event)).is_err()
        {
            break;
        }
    }
}
