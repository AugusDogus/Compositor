//! Lightweight foreground-process inspection for PTY-backed terminal metadata.

use std::path::Path;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DetectedAgentProcess {
    pub(crate) kind: &'static str,
    pub(crate) process_id: u32,
}

#[derive(Debug)]
struct ProcessInfo {
    process_id: u32,
    name: String,
    arguments: Vec<String>,
}

pub(crate) fn detect_agent_process(shell_process_id: u32) -> Option<DetectedAgentProcess> {
    let foreground_group = foreground_group_id(shell_process_id)?;
    let mut processes = foreground_processes(shell_process_id);
    processes.sort_by_key(|process| process.process_id != foreground_group);
    processes.into_iter().find_map(|process| {
        identify_agent(&process).map(|kind| DetectedAgentProcess {
            kind,
            process_id: process.process_id,
        })
    })
}

fn identify_agent(process: &ProcessInfo) -> Option<&'static str> {
    exact_agent_name(&process.name).or_else(|| {
        process.arguments.iter().find_map(|argument| {
            exact_agent_name(argument).or_else(|| agent_from_package_path(argument))
        })
    })
}

fn normalized_basename(value: &str) -> String {
    Path::new(value)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(value)
        .trim_start_matches('-')
        .trim_end_matches(".exe")
        .to_ascii_lowercase()
}

fn exact_agent_name(value: &str) -> Option<&'static str> {
    match normalized_basename(value).as_str() {
        "pi" => Some("pi"),
        "claude" | "claude-code" => Some("claude"),
        "codex" => Some("codex"),
        "gemini" => Some("gemini"),
        "cursor" | "cursor-agent" => Some("cursor"),
        "devin" | "devin-cli" => Some("devin"),
        "agy" | "antigravity" | "antigravity-cli" => Some("agy"),
        "cline" => Some("cline"),
        "omp" => Some("omp"),
        "mastracode" | "mastra-code" => Some("mastracode"),
        "opencode" | "opencode2" | "open-code" => Some("opencode"),
        "copilot" | "github-copilot" | "ghcs" => Some("copilot"),
        "kimi" | "kimi-code" => Some("kimi"),
        "kiro" | "kiro-cli" => Some("kiro"),
        "droid" => Some("droid"),
        "amp" | "amp-local" => Some("amp"),
        "grok" | "grok-build" => Some("grok"),
        "hermes" | "hermes-agent" => Some("hermes"),
        "kilo" | "kilo-code" => Some("kilo"),
        "qodercli" | "qoderclicn" | "qoder" | "qodercn" => Some("qodercli"),
        "qwen" | "qwen-code" => Some("qwen"),
        "maki" => Some("maki"),
        "muse" | "muse-code" | "muse-cli" => Some("muse"),
        value
            if value.strip_prefix("muse-bin-").is_some_and(|suffix| {
                suffix.starts_with(|character: char| character.is_ascii_digit())
            }) =>
        {
            Some("muse")
        }
        _ => None,
    }
}

fn agent_from_package_path(value: &str) -> Option<&'static str> {
    let normalized = value.to_ascii_lowercase().replace('\\', "/");
    if normalized.contains("@anthropic-ai/claude-code") {
        Some("claude")
    } else if normalized.contains("@openai/codex") {
        Some("codex")
    } else if normalized.contains("/opencode-ai/") || normalized.contains("/opencode/") {
        Some("opencode")
    } else if normalized.contains("@google/gemini-cli") {
        Some("gemini")
    } else if normalized.contains("@github/copilot") {
        Some("copilot")
    } else {
        None
    }
}

#[cfg(target_os = "macos")]
fn foreground_group_id(process_id: u32) -> Option<u32> {
    let info = process_bsd_info(process_id)?;
    (info.e_tpgid > 0).then_some(info.e_tpgid)
}

#[cfg(target_os = "macos")]
fn foreground_processes(shell_process_id: u32) -> Vec<ProcessInfo> {
    const PROCESS_GROUP_ONLY: u32 = 2;

    let Some(group_id) = foreground_group_id(shell_process_id) else {
        return Vec::new();
    };
    let mut capacity = 16_usize;
    for _ in 0..8 {
        let mut process_ids = vec![0 as libc::pid_t; capacity];
        let byte_capacity = process_ids.len() * std::mem::size_of::<libc::pid_t>();
        let returned_bytes = unsafe {
            libc::proc_listpids(
                PROCESS_GROUP_ONLY,
                group_id,
                process_ids.as_mut_ptr().cast(),
                byte_capacity as libc::c_int,
            )
        };
        if returned_bytes <= 0 {
            return Vec::new();
        }
        if returned_bytes as usize >= byte_capacity {
            capacity = capacity.saturating_mul(2);
            continue;
        }
        return process_ids
            .into_iter()
            .take(returned_bytes as usize / std::mem::size_of::<libc::pid_t>())
            .filter_map(|process_id| u32::try_from(process_id).ok())
            .filter(|process_id| *process_id > 0)
            .filter_map(|process_id| {
                let info = process_bsd_info(process_id)?;
                if info.pbi_pgid != group_id {
                    return None;
                }
                let name = process_name(&info)?;
                Some(ProcessInfo {
                    process_id,
                    name,
                    arguments: process_arguments(process_id).unwrap_or_default(),
                })
            })
            .collect();
    }
    Vec::new()
}

#[cfg(target_os = "macos")]
fn process_bsd_info(process_id: u32) -> Option<libc::proc_bsdinfo> {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    let returned = unsafe {
        libc::proc_pidinfo(
            process_id as libc::c_int,
            libc::PROC_PIDTBSDINFO,
            0,
            &mut info as *mut _ as *mut libc::c_void,
            size,
        )
    };
    (returned == size).then_some(info)
}

#[cfg(target_os = "macos")]
fn process_name(info: &libc::proc_bsdinfo) -> Option<String> {
    let end = info
        .pbi_comm
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(info.pbi_comm.len());
    if end == 0 {
        return None;
    }
    let bytes = info.pbi_comm[..end]
        .iter()
        .map(|byte| *byte as u8)
        .collect::<Vec<_>>();
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(target_os = "macos")]
fn process_arguments(process_id: u32) -> Option<Vec<String>> {
    let mut query = [
        libc::CTL_KERN,
        libc::KERN_PROCARGS2,
        process_id as libc::c_int,
    ];
    let mut size: libc::size_t = 0;
    if unsafe {
        libc::sysctl(
            query.as_mut_ptr(),
            3,
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    } != 0
        || size < 4
    {
        return None;
    }
    let mut bytes = vec![0_u8; size];
    if unsafe {
        libc::sysctl(
            query.as_mut_ptr(),
            3,
            bytes.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    } != 0
    {
        return None;
    }
    bytes.truncate(size);
    let argument_count = i32::from_ne_bytes(bytes[..4].try_into().ok()?);
    if argument_count < 1 {
        return None;
    }
    let rest = &bytes[4..];
    let executable_end = rest.iter().position(|byte| *byte == 0)?;
    let mut cursor = executable_end;
    while cursor < rest.len() && rest[cursor] == 0 {
        cursor += 1;
    }
    let mut arguments = Vec::with_capacity(argument_count as usize);
    for _ in 0..argument_count {
        let end = rest.get(cursor..)?.iter().position(|byte| *byte == 0)? + cursor;
        if end == cursor {
            return None;
        }
        arguments.push(String::from_utf8_lossy(&rest[cursor..end]).into_owned());
        cursor = end.saturating_add(1);
    }
    Some(arguments)
}

#[cfg(target_os = "linux")]
fn foreground_group_id(process_id: u32) -> Option<u32> {
    linux_process_stat(process_id).and_then(|(_, group_id)| u32::try_from(group_id).ok())
}

#[cfg(target_os = "linux")]
fn foreground_processes(shell_process_id: u32) -> Vec<ProcessInfo> {
    let Some(group_id) = foreground_group_id(shell_process_id) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| entry.file_name().to_string_lossy().parse::<u32>().ok())
        .filter(|process_id| {
            linux_process_stat(*process_id).is_some_and(|(group, _)| group == group_id as i32)
        })
        .filter_map(|process_id| {
            let name = std::fs::read_to_string(format!("/proc/{process_id}/comm"))
                .ok()?
                .trim()
                .to_owned();
            let arguments = std::fs::read(format!("/proc/{process_id}/cmdline"))
                .ok()
                .map(|bytes| {
                    bytes
                        .split(|byte| *byte == 0)
                        .filter(|part| !part.is_empty())
                        .map(|part| String::from_utf8_lossy(part).into_owned())
                        .collect()
                })
                .unwrap_or_default();
            Some(ProcessInfo {
                process_id,
                name,
                arguments,
            })
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn linux_process_stat(process_id: u32) -> Option<(i32, i32)> {
    let stat = std::fs::read_to_string(format!("/proc/{process_id}/stat")).ok()?;
    let fields = stat
        .get(stat.rfind(')')?.saturating_add(2)..)?
        .split_whitespace()
        .collect::<Vec<_>>();
    let group_id = fields.get(2)?.parse().ok()?;
    let foreground_group_id = fields.get(5)?.parse().ok()?;
    Some((group_id, foreground_group_id))
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn foreground_group_id(_process_id: u32) -> Option<u32> {
    None
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn foreground_processes(_shell_process_id: u32) -> Vec<ProcessInfo> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn process(name: &str, arguments: &[&str]) -> ProcessInfo {
        ProcessInfo {
            process_id: 42,
            name: name.to_owned(),
            arguments: arguments.iter().map(|value| (*value).to_owned()).collect(),
        }
    }

    #[test]
    fn recognizes_native_and_node_agent_processes() {
        assert_eq!(identify_agent(&process("codex", &[])), Some("codex"));
        assert_eq!(
            identify_agent(&process(
                "node",
                &["node", "/opt/node_modules/@anthropic-ai/claude-code/cli.js"]
            )),
            Some("claude")
        );
        assert_eq!(identify_agent(&process("zsh", &["-zsh"])), None);
    }
}
