use super::*;
use std::{
    io::Write,
    os::fd::AsFd,
    sync::atomic::{AtomicUsize, Ordering},
    time::Instant,
};

pub(super) struct Permit(Arc<AtomicUsize>);

impl Permit {
    pub fn acquire(active: &Arc<AtomicUsize>) -> Result<Self> {
        active.fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| (count < 4).then_some(count + 1))
            .map_err(|_| invalid("Four clipboard transfers are already active. Wait for them to finish and copy again."))?;
        Ok(Self(active.clone()))
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

pub(super) fn write(mut pipe: impl Write + AsFd, bytes: &[u8]) -> Result<()> {
    use rustix::{
        event::{PollFd, PollFlags, Timespec, poll},
        fs::{OFlags, fcntl_getfl, fcntl_setfl},
    };
    let flags = fcntl_getfl(&pipe).map_err(std::io::Error::from)?;
    fcntl_setfl(&pipe, flags | OFlags::NONBLOCK).map_err(std::io::Error::from)?;
    let deadline = Instant::now() + super::super::TIMEOUT;
    let mut written = 0;
    while written < bytes.len() {
        super::super::check_deadline(deadline)?;
        match pipe.write(&bytes[written..]) {
            Ok(0) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::WriteZero,
                    "Clipboard receiver stopped reading",
                )
                .into());
            }
            Ok(count) => written += count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                let left = deadline.saturating_duration_since(Instant::now());
                let timeout = Timespec {
                    tv_sec: left.as_secs() as i64,
                    tv_nsec: left.subsec_nanos() as i64,
                };
                match poll(&mut [PollFd::new(&pipe, PollFlags::OUT)], Some(&timeout)) {
                    Ok(_) | Err(rustix::io::Errno::INTR) => {}
                    Err(error) => return Err(std::io::Error::from(error).into()),
                }
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Read, os::unix::net::UnixStream};

    #[test]
    fn transfer_limit_releases_slots_on_drop() {
        let active = Arc::new(AtomicUsize::new(0));
        let mut permits: Vec<_> = (0..4).map(|_| Permit::acquire(&active).unwrap()).collect();
        assert!(Permit::acquire(&active).is_err());
        permits.pop();
        assert!(Permit::acquire(&active).is_ok());
        drop(permits);
        assert_eq!(active.load(Ordering::Acquire), 0);
    }

    #[test]
    fn writer_transfers_beyond_pipe_capacity_and_reports_closed_receivers() {
        let bytes = vec![37; 2 * 1024 * 1024];
        let (writer, mut reader) = UnixStream::pair().unwrap();
        let receiver = std::thread::spawn(move || {
            let mut received = Vec::new();
            reader.read_to_end(&mut received).unwrap();
            received
        });
        write(writer, &bytes).unwrap();
        assert_eq!(receiver.join().unwrap(), bytes);
        let (writer, reader) = UnixStream::pair().unwrap();
        drop(reader);
        assert!(write(writer, &[1]).is_err());
    }
}
