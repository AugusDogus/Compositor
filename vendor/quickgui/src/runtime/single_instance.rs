use super::*;
use web_time::{Duration, Instant};

use std::{
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
};

use interprocess::{
    ConnectWaitMode,
    local_socket::{
        ConnectOptions, GenericFilePath, ListenerOptions,
        prelude::{LocalSocketListener, LocalSocketStream, *},
    },
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[cfg(any(target_os = "linux", target_os = "openbsd"))]
use interprocess::os::unix::local_socket::ListenerOptionsExt;

/// Maximum UTF-8 bytes accepted for a single-instance application identifier.
pub const MAX_SINGLE_INSTANCE_IDENTIFIER_BYTES: usize = 256;
/// Maximum arguments forwarded by a later application process.
pub const MAX_SECOND_INSTANCE_ARGUMENTS: usize = 4_096;
/// Maximum encoded size of one second-instance message.
pub const MAX_SECOND_INSTANCE_MESSAGE_BYTES: usize = 1024 * 1024;

const SINGLE_INSTANCE_PROTOCOL_VERSION: u8 = 1;
const SINGLE_INSTANCE_IO_TIMEOUT: Duration = Duration::from_secs(2);

/// Arguments and working directory forwarded by a later application process.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SecondInstanceEvent {
    argv: Arc<[Arc<str>]>,
    cwd: PathBuf,
}

impl SecondInstanceEvent {
    pub fn argv(&self) -> &[Arc<str>] {
        &self.argv
    }

    pub fn cwd(&self) -> &Path {
        &self.cwd
    }
}

/// Failure to acquire or contact an application single-instance endpoint.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum SingleInstanceError {
    #[error(
        "a single-instance identifier must contain 3-256 ASCII letters, digits, dots, dashes, or underscores"
    )]
    InvalidIdentifier,
    #[error("the second-instance payload exceeds QuickGUI's bounded IPC limits")]
    PayloadTooLarge,
    #[error("this application already owns a different single-instance identifier")]
    IdentifierAlreadySet,
    #[error("single-instance IPC failed: {0}")]
    Ipc(Arc<str>),
}

#[derive(Serialize, Deserialize)]
struct WireSecondInstance {
    version: u8,
    argv: Vec<String>,
    cwd: String,
}

pub(super) struct SingleInstanceGuard {
    identifier: Arc<str>,
    endpoint: PathBuf,
    stopping: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl SingleInstanceGuard {
    fn identifier(&self) -> &str {
        &self.identifier
    }
}

impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Release);
        if let Ok(mut stream) = connect(&self.endpoint) {
            let _ = write_all_bounded(&mut stream, &0_u32.to_le_bytes());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

enum SingleInstanceAcquisition {
    Primary(SingleInstanceGuard),
    Secondary,
}

impl AppRunner {
    pub const fn single_instance_supported(&self) -> bool {
        DesktopIntegrationSupport::current().single_instance
    }

    pub fn has_single_instance_lock(&self) -> bool {
        self.runtime.single_instance.is_some()
    }

    pub fn single_instance_identifier(&self) -> Option<&str> {
        self.runtime
            .single_instance
            .as_ref()
            .map(SingleInstanceGuard::identifier)
    }

    /// Acquire the process-local endpoint for an application identifier.
    ///
    /// The first process returns `true` and receives later launches through
    /// [`Application::on_second_instance`]. A later process forwards its arguments and working directory,
    /// waits for acknowledgement, and returns `false`.
    pub fn request_single_instance_lock(
        &mut self,
        identifier: impl Into<Arc<str>>,
    ) -> Result<bool, SingleInstanceError> {
        let identifier = identifier.into();
        validate_identifier(&identifier)?;
        if let Some(instance) = &self.runtime.single_instance {
            return if instance.identifier() == identifier.as_ref() {
                Ok(true)
            } else {
                Err(SingleInstanceError::IdentifierAlreadySet)
            };
        }
        let message = current_instance_message()?;
        match acquire_single_instance(identifier, message, self.runtime.event_proxy.clone())? {
            SingleInstanceAcquisition::Primary(instance) => {
                self.runtime.single_instance = Some(instance);
                Ok(true)
            }
            SingleInstanceAcquisition::Secondary => Ok(false),
        }
    }

    /// Release this process's single-instance endpoint, if one is owned.
    pub fn release_single_instance_lock(&mut self) -> bool {
        self.runtime.single_instance.take().is_some()
    }
}

impl Runtime {
    pub(super) fn invoke_second_instance(
        &mut self,
        event_loop: &ActiveEventLoop,
        event: SecondInstanceEvent,
    ) {
        let urls = super::deep_link::open_urls_from_arguments(event.argv(), event.cwd());
        let mut open_urls = self.application_callbacks.open_urls.take().zip(urls);
        let mut second_instance = self.application_callbacks.second_instance.take();
        if open_urls.is_none() && second_instance.is_none() {
            return;
        }
        let mut context = self.event_context();
        if let Some((callback, urls)) = &mut open_urls {
            callback(urls.clone(), &mut context);
        }
        if let Some(callback) = &mut second_instance {
            callback(event, &mut context);
        }
        if let Some((callback, _)) = open_urls {
            self.application_callbacks.open_urls = Some(callback);
        }
        self.application_callbacks.second_instance = second_instance;
        self.apply_application_context(event_loop, context);
    }
}

fn acquire_single_instance(
    identifier: Arc<str>,
    message: WireSecondInstance,
    proxy: EventLoopProxy<RuntimeEvent>,
) -> Result<SingleInstanceAcquisition, SingleInstanceError> {
    let endpoint = endpoint_for(&identifier);
    match create_listener(&endpoint, false) {
        Ok(listener) => start_primary(identifier, endpoint, listener, proxy),
        Err(error) if error.kind() == io::ErrorKind::AddrInUse => {
            match forward_message(&endpoint, &message) {
                Ok(()) => Ok(SingleInstanceAcquisition::Secondary),
                #[cfg(unix)]
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
                    ) =>
                {
                    let listener = create_listener(&endpoint, true).map_err(ipc_error)?;
                    start_primary(identifier, endpoint, listener, proxy)
                }
                Err(error) => Err(ipc_error(error)),
            }
        }
        Err(error) => Err(ipc_error(error)),
    }
}

fn start_primary(
    identifier: Arc<str>,
    endpoint: PathBuf,
    listener: LocalSocketListener,
    proxy: EventLoopProxy<RuntimeEvent>,
) -> Result<SingleInstanceAcquisition, SingleInstanceError> {
    let stopping = Arc::new(AtomicBool::new(false));
    let thread_stopping = Arc::clone(&stopping);
    let thread = thread::Builder::new()
        .name("quickgui-single-instance".to_owned())
        .spawn(move || listen_for_second_instances(listener, proxy, thread_stopping))
        .map_err(ipc_error)?;
    Ok(SingleInstanceAcquisition::Primary(SingleInstanceGuard {
        identifier,
        endpoint,
        stopping,
        thread: Some(thread),
    }))
}

fn listen_for_second_instances(
    listener: LocalSocketListener,
    proxy: EventLoopProxy<RuntimeEvent>,
    stopping: Arc<AtomicBool>,
) {
    loop {
        let mut stream = match listener.accept() {
            Ok(stream) => stream,
            Err(error) => {
                if !stopping.load(Ordering::Acquire) {
                    tracing::warn!(%error, "single-instance IPC accept failed");
                }
                break;
            }
        };
        if stopping.load(Ordering::Acquire) {
            break;
        }
        match read_message(&mut stream) {
            Ok(None) => break,
            Ok(Some(event)) => {
                let delivered = proxy
                    .send_event(RuntimeEvent::SecondInstance(event))
                    .is_ok();
                let _ = write_all_bounded(&mut stream, &[u8::from(delivered)]);
                if !delivered {
                    break;
                }
            }
            Err(error) => {
                tracing::warn!(%error, "discarded invalid single-instance IPC message");
                let _ = write_all_bounded(&mut stream, &[0]);
            }
        }
    }
}

fn current_instance_message() -> Result<WireSecondInstance, SingleInstanceError> {
    let argv = std::env::args_os()
        .skip(1)
        .take(MAX_SECOND_INSTANCE_ARGUMENTS + 1)
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    if argv.len() > MAX_SECOND_INSTANCE_ARGUMENTS {
        return Err(SingleInstanceError::PayloadTooLarge);
    }
    let cwd = std::env::current_dir()
        .map_err(ipc_error)?
        .to_string_lossy()
        .into_owned();
    let message = WireSecondInstance {
        version: SINGLE_INSTANCE_PROTOCOL_VERSION,
        argv,
        cwd,
    };
    encode_message(&message)?;
    Ok(message)
}

fn forward_message(endpoint: &Path, message: &WireSecondInstance) -> io::Result<()> {
    let payload = encode_message(message).map_err(single_instance_io_error)?;
    let length = u32::try_from(payload.len())
        .map_err(|_| single_instance_io_error(SingleInstanceError::PayloadTooLarge))?;
    let mut stream = connect(endpoint)?;
    write_all_bounded(&mut stream, &length.to_le_bytes())?;
    write_all_bounded(&mut stream, &payload)?;
    let mut acknowledgement = [0_u8; 1];
    read_exact_bounded(&mut stream, &mut acknowledgement)?;
    if acknowledgement[0] == 1 {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::ConnectionAborted,
            "the primary application rejected the second-instance message",
        ))
    }
}

fn read_message(stream: &mut LocalSocketStream) -> io::Result<Option<SecondInstanceEvent>> {
    let mut length = [0_u8; 4];
    read_exact_bounded(stream, &mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    if length == 0 {
        return Ok(None);
    }
    if length > MAX_SECOND_INSTANCE_MESSAGE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            SingleInstanceError::PayloadTooLarge,
        ));
    }
    let mut payload = vec![0_u8; length];
    read_exact_bounded(stream, &mut payload)?;
    let wire = serde_json::from_slice::<WireSecondInstance>(&payload)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    validate_wire_message(wire)
        .map(Some)
        .map_err(single_instance_io_error)
}

fn validate_wire_message(
    message: WireSecondInstance,
) -> Result<SecondInstanceEvent, SingleInstanceError> {
    if message.version != SINGLE_INSTANCE_PROTOCOL_VERSION
        || message.argv.len() > MAX_SECOND_INSTANCE_ARGUMENTS
    {
        return Err(SingleInstanceError::PayloadTooLarge);
    }
    let argv_bytes = message.argv.iter().map(String::len).sum::<usize>();
    if argv_bytes.saturating_add(message.cwd.len()) > MAX_SECOND_INSTANCE_MESSAGE_BYTES {
        return Err(SingleInstanceError::PayloadTooLarge);
    }
    Ok(SecondInstanceEvent {
        argv: message.argv.into_iter().map(Arc::<str>::from).collect(),
        cwd: PathBuf::from(message.cwd),
    })
}

fn encode_message(message: &WireSecondInstance) -> Result<Vec<u8>, SingleInstanceError> {
    let payload = serde_json::to_vec(message).map_err(ipc_error)?;
    if payload.len() > MAX_SECOND_INSTANCE_MESSAGE_BYTES {
        return Err(SingleInstanceError::PayloadTooLarge);
    }
    Ok(payload)
}

fn validate_identifier(identifier: &str) -> Result<(), SingleInstanceError> {
    let valid = (3..=MAX_SINGLE_INSTANCE_IDENTIFIER_BYTES).contains(&identifier.len())
        && identifier.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphanumeric() || (index > 0 && b"._-".contains(&byte))
        });
    if valid {
        Ok(())
    } else {
        Err(SingleInstanceError::InvalidIdentifier)
    }
}

fn endpoint_for(identifier: &str) -> PathBuf {
    let hash = stable_identifier_hash(identifier.as_bytes());
    #[cfg(windows)]
    {
        return PathBuf::from(format!(r"\\.\pipe\quickgui-{hash:016x}"));
    }
    #[cfg(unix)]
    {
        std::env::temp_dir().join(format!("quickgui-{hash:016x}.sock"))
    }
}

fn stable_identifier_hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325_u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

fn create_listener(endpoint: &Path, overwrite: bool) -> io::Result<LocalSocketListener> {
    let name = endpoint.to_path_buf().to_fs_name::<GenericFilePath>()?;
    let options = ListenerOptions::new()
        .name(name)
        .reclaim_name(true)
        .try_overwrite(overwrite)
        .max_spin_time(Duration::from_millis(100))
        .nonblocking(interprocess::local_socket::ListenerNonblockingMode::Stream);
    // Interprocess implements this without a process-wide umask race only on platforms whose
    // socket implementation accepts fchmod before bind. macOS rejects it as unsupported.
    #[cfg(any(target_os = "linux", target_os = "openbsd"))]
    let options = options.mode(0o600);
    options.create_sync()
}

fn connect(endpoint: &Path) -> io::Result<LocalSocketStream> {
    let name = endpoint.to_path_buf().to_fs_name::<GenericFilePath>()?;
    ConnectOptions::new()
        .name(name)
        .wait_mode(ConnectWaitMode::Timeout(SINGLE_INSTANCE_IO_TIMEOUT))
        .nonblocking_stream(true)
        .connect_sync()
}

fn read_exact_bounded(stream: &mut LocalSocketStream, output: &mut [u8]) -> io::Result<()> {
    let deadline = Instant::now() + SINGLE_INSTANCE_IO_TIMEOUT;
    let mut offset = 0;
    while offset < output.len() {
        match stream.read(&mut output[offset..]) {
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(read) => offset += read,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err(io::ErrorKind::TimedOut.into());
                }
                thread::sleep(Duration::from_millis(2));
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn write_all_bounded(stream: &mut LocalSocketStream, input: &[u8]) -> io::Result<()> {
    let deadline = Instant::now() + SINGLE_INSTANCE_IO_TIMEOUT;
    let mut offset = 0;
    while offset < input.len() {
        match stream.write(&input[offset..]) {
            Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
            Ok(written) => offset += written,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err(io::ErrorKind::TimedOut.into());
                }
                thread::sleep(Duration::from_millis(2));
            }
            Err(error) => return Err(error),
        }
    }
    stream.flush()
}

fn ipc_error(error: impl std::fmt::Display) -> SingleInstanceError {
    SingleInstanceError::Ipc(Arc::from(error.to_string()))
}

fn single_instance_io_error(error: SingleInstanceError) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_are_bounded_and_endpoint_safe() {
        assert!(validate_identifier("com.example.app").is_ok());
        assert!(validate_identifier("ab").is_err());
        assert!(validate_identifier("-example").is_err());
        assert!(validate_identifier("contains space").is_err());
        assert!(validate_identifier(&"a".repeat(257)).is_err());
        assert_eq!(
            endpoint_for("com.example.app"),
            endpoint_for("com.example.app")
        );
        assert_ne!(
            endpoint_for("com.example.app"),
            endpoint_for("com.example.other")
        );
    }

    #[test]
    fn wire_messages_enforce_limits() {
        let message = WireSecondInstance {
            version: SINGLE_INSTANCE_PROTOCOL_VERSION,
            argv: vec!["app".to_owned(), "quickgui://open".to_owned()],
            cwd: "/tmp".to_owned(),
        };
        let event = validate_wire_message(message).expect("valid message");
        assert_eq!(event.argv()[1].as_ref(), "quickgui://open");
        assert_eq!(event.cwd(), Path::new("/tmp"));
    }

    #[test]
    fn platform_endpoint_supports_a_local_socket_round_trip() {
        let nonce = web_time::SystemTime::now()
            .duration_since(web_time::UNIX_EPOCH)
            .expect("system clock after Unix epoch")
            .as_nanos();
        let endpoint = endpoint_for(&format!(
            "quickgui.single-instance-test.{}.{nonce}",
            std::process::id()
        ));
        let listener = create_listener(&endpoint, false).expect("create local socket listener");
        let mut client = connect(&endpoint).expect("connect to local socket listener");
        let mut server = listener.accept().expect("accept local socket stream");

        write_all_bounded(&mut client, b"q").expect("write local socket payload");
        let mut payload = [0_u8; 1];
        read_exact_bounded(&mut server, &mut payload).expect("read local socket payload");
        assert_eq!(payload, *b"q");
    }
}
