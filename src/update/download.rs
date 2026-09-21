use crate::{Result, invalid};
use quickgui::UpdateCancellation;
use std::{
    io::{Read, Write},
    time::Duration,
};

pub(super) fn receive(
    url: &str,
    output: &mut impl Write,
    maximum: u64,
    seconds: u64,
    cancellation: &UpdateCancellation,
) -> Result<()> {
    check_cancelled(cancellation)?;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .https_only(true)
        .timeout_global(Some(Duration::from_secs(seconds)))
        .timeout_resolve(Some(Duration::from_secs(10)))
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_recv_response(Some(Duration::from_secs(15)))
        .build()
        .into();
    let mut response = agent.get(url).call().map_err(|e| invalid(e.to_string()))?;
    copy(
        response.body_mut().as_reader(),
        output,
        maximum,
        cancellation,
    )
}

fn copy(
    mut input: impl Read,
    output: &mut impl Write,
    maximum: u64,
    cancellation: &UpdateCancellation,
) -> Result<()> {
    let mut buffer = [0; 64 * 1024];
    let mut received = 0_u64;
    loop {
        check_cancelled(cancellation)?;
        let count = input.read(&mut buffer)?;
        check_cancelled(cancellation)?;
        if count == 0 {
            return Ok(());
        }
        received += count as u64;
        if received > maximum {
            return Err(invalid(format!(
                "The update response exceeds the {maximum}-byte limit."
            )));
        }
        output.write_all(&buffer[..count])?;
    }
}

fn check_cancelled(cancellation: &UpdateCancellation) -> Result<()> {
    if cancellation.is_cancelled() {
        Err(invalid("Update request cancelled"))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn downloads_are_bounded_and_cancellation_prevents_writes() {
        let cancellation = UpdateCancellation::new();
        let mut output = Vec::new();
        copy(&b"update"[..], &mut output, 6, &cancellation).unwrap();
        assert_eq!(output, b"update");
        assert!(copy(&b"oversized"[..], &mut Vec::new(), 6, &cancellation).is_err());
        cancellation.cancel();
        let mut output = Vec::new();
        assert!(copy(&b"update"[..], &mut output, 6, &cancellation).is_err());
        assert!(output.is_empty());
    }
}
