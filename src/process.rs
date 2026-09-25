use std::{fs, path::PathBuf};

#[derive(Clone, Debug, Default)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub command: Option<String>,
    pub cwd: Option<PathBuf>,
    pub uid: Option<u32>,
    pub start_time: Option<u64>,
    pub systemd_service: bool,
    pub ancestors: Vec<ProcessSnapshot>,
}

#[derive(Clone, Debug)]
pub struct ProcessSnapshot {
    pub pid: u32,
    pub name: String,
    pub command: Option<String>,
    pub cwd: Option<PathBuf>,
    pub start_time: Option<u64>,
}

pub fn inspect(pid: u32) -> Option<ProcessInfo> {
    let process = snapshot(pid)?;
    let uid = fs::read_to_string(format!("/proc/{pid}/status"))
        .ok()
        .and_then(|status| parse_uid(&status));

    let mut ancestors = Vec::new();
    let mut parent = parent_pid(pid);
    for _ in 0..8 {
        let Some(parent_id) = parent else { break };
        let Some(snapshot) = snapshot(parent_id) else {
            break;
        };
        ancestors.push(snapshot);
        parent = parent_pid(parent_id);
    }

    Some(ProcessInfo {
        pid: process.pid,
        name: process.name,
        command: process.command,
        cwd: process.cwd,
        uid,
        start_time: process.start_time,
        systemd_service: fs::read_to_string(format!("/proc/{pid}/cgroup"))
            .is_ok_and(|cgroup| cgroup.lines().any(|line| line.contains(".service"))),
        ancestors,
    })
}

fn snapshot(pid: u32) -> Option<ProcessSnapshot> {
    let base = PathBuf::from(format!("/proc/{pid}"));
    if !base.exists() {
        return None;
    }
    let name = fs::read_to_string(base.join("comm"))
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_owned());
    let command = fs::read(base.join("cmdline"))
        .ok()
        .and_then(|bytes| parse_cmdline(&bytes));
    let cwd = fs::read_link(base.join("cwd")).ok();
    let start_time = fs::read_to_string(base.join("stat"))
        .ok()
        .and_then(|stat| parse_start_time(&stat));
    Some(ProcessSnapshot {
        pid,
        name,
        command,
        cwd,
        start_time,
    })
}

fn parse_start_time(stat: &str) -> Option<u64> {
    let end_of_name = stat.rfind(')')?;
    stat.get(end_of_name + 1..)?
        .split_whitespace()
        .nth(19)?
        .parse()
        .ok()
}

pub fn user_name(uid: u32) -> Option<String> {
    fs::read_to_string("/etc/passwd")
        .ok()?
        .lines()
        .find_map(|line| {
            let mut fields = line.split(':');
            let name = fields.next()?;
            let _password = fields.next()?;
            let user_id = fields.next()?.parse::<u32>().ok()?;
            (user_id == uid).then(|| name.to_owned())
        })
}

fn parent_pid(pid: u32) -> Option<u32> {
    let status = fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    parse_parent_pid(&status)
}

fn parse_uid(status: &str) -> Option<u32> {
    status.lines().find_map(|line| {
        line.strip_prefix("Uid:")
            .and_then(|value| value.split_whitespace().next())
            .and_then(|value| value.parse().ok())
    })
}

fn parse_cmdline(bytes: &[u8]) -> Option<String> {
    let command = bytes
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8_lossy(part).into_owned())
        .collect::<Vec<_>>()
        .join(" ");
    (!command.is_empty()).then_some(command)
}

fn parse_parent_pid(status: &str) -> Option<u32> {
    status.lines().find_map(|line| {
        line.strip_prefix("PPid:")
            .and_then(|value| value.trim().parse().ok())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_parent_pid_from_proc_status() {
        assert_eq!(
            parse_parent_pid("Name:\tnode\nState:\tS\nPPid:\t3012\n"),
            Some(3012)
        );
    }

    #[test]
    fn parses_uid_from_proc_status() {
        let status = "Name:\tnode\nUid:\t1000\t1000\t1000\t1000\n";
        assert_eq!(parse_uid(status), Some(1000));
    }

    #[test]
    fn parses_null_terminated_command_line_arguments() {
        assert_eq!(
            parse_cmdline(b"node\0server.js\0--port=5173\0").as_deref(),
            Some("node server.js --port=5173")
        );
        assert_eq!(parse_cmdline(b"\0\0"), None);
    }

    #[test]
    fn parses_process_start_time_after_a_comm_field_with_spaces() {
        let mut stat = "42 (a process name) S 1".to_owned();
        for _ in 0..17 {
            stat.push_str(" 0");
        }
        stat.push_str(" 987654 0 0");
        assert_eq!(parse_start_time(&stat), Some(987654));
    }
}
