use crate::{config::Config, models::Service, process, scanner};
use anyhow::{Context, bail};
use std::{
    io::{self, IsTerminal, Write},
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

pub fn service_on_port(config: &Config, port: u16) -> anyhow::Result<Service> {
    scanner::discover_services(config)
        .into_iter()
        .find(|service| service.port == port)
        .with_context(|| format!("no listening service found on port {port}"))
}

pub fn open_browser(service: &Service) -> anyhow::Result<()> {
    let url = service
        .url
        .as_deref()
        .with_context(|| format!("port {} does not look like an HTTP service", service.port))?;
    Command::new("xdg-open")
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("could not start xdg-open; install xdg-utils or open the URL manually")?;
    Ok(())
}

pub fn open_project(config: &Config, service: &Service, editor: bool) -> anyhow::Result<()> {
    let project = service
        .project
        .as_ref()
        .with_context(|| format!("could not resolve a project for port {}", service.port))?;
    if project.root.is_empty() {
        bail!("the project name is known, but no host working directory is available");
    }
    let configured = if editor {
        &config.actions.editor
    } else {
        &config.actions.terminal
    };
    let program = find_program(configured).with_context(|| {
        format!(
            "configured {} '{}' was not found in PATH",
            if editor { "editor" } else { "terminal" },
            configured
        )
    })?;
    let configured_name = Path::new(configured)
        .file_name()
        .and_then(|part| part.to_str())
        .unwrap_or(configured);
    if editor && is_terminal_editor(configured_name) {
        let terminal = find_program(&config.actions.terminal).with_context(|| {
            format!(
                "configured terminal '{}' was not found in PATH",
                config.actions.terminal
            )
        })?;
        let mut command = Command::new(terminal);
        let terminal_name = Path::new(&config.actions.terminal)
            .file_name()
            .and_then(|part| part.to_str())
            .unwrap_or(&config.actions.terminal);
        match terminal_name {
            "kitty" => command.args([
                "--directory",
                &project.root,
                "--",
                configured,
                &project.root,
            ]),
            "foot" => command.args([
                "--working-directory",
                &project.root,
                configured,
                &project.root,
            ]),
            "alacritty" => command.args([
                "--working-directory",
                &project.root,
                "-e",
                configured,
                &project.root,
            ]),
            "ghostty" => command.args([
                "--working-directory",
                &project.root,
                "-e",
                configured,
                &project.root,
            ]),
            _ => command.args([
                "--working-directory",
                &project.root,
                "-e",
                configured,
                &project.root,
            ]),
        };
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| {
                format!(
                    "could not launch {} inside {}",
                    configured, config.actions.terminal
                )
            })?;
        return Ok(());
    }
    let mut command = Command::new(program);
    if editor {
        command.arg(&project.root);
    } else {
        if configured_name == "kitty" {
            command.args(["--directory", &project.root]);
        } else if configured_name == "foot" || configured_name == "alacritty" {
            command.args(["--working-directory", &project.root]);
        } else if configured_name == "ghostty" {
            command.arg(format!("--working-directory={}", project.root));
        } else {
            command.args(["--working-directory", &project.root]);
        }
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("could not launch {}", configured))?;
    Ok(())
}

fn is_terminal_editor(name: &str) -> bool {
    [
        "nvim", "vim", "vi", "hx", "helix", "nano", "micro", "kak", "kakoune", "joe",
    ]
    .contains(&name)
}

pub fn confirm_kill(service: &Service, force: bool, yes: bool) -> anyhow::Result<bool> {
    if yes {
        return Ok(true);
    }
    if !io::stdin().is_terminal() {
        bail!("kill requires confirmation; rerun with --yes after reviewing the service details");
    }
    println!("{}", kill_summary(service));
    print!(
        "{} this service? [y/N] ",
        if force { "Force kill" } else { "Stop" }
    );
    io::stdout().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

pub fn kill_service(config: &Config, service: &Service, force: bool) -> anyhow::Result<String> {
    if let Some(docker) = &service.docker {
        if force {
            run_bounded(
                "docker",
                &["kill", &docker.container_id],
                Duration::from_secs(4),
            )
            .context("Docker could not force-stop the container")?;
            return Ok(format!("Force-stopped container {}", docker.container));
        }
        run_bounded(
            "docker",
            &["stop", "--time", "5", &docker.container_id],
            Duration::from_secs(7),
        )
        .context("Docker could not stop the container")?;
        return Ok(format!("Stopped container {}", docker.container));
    }

    let pid = service
        .pid
        .with_context(|| format!("no accessible process owns port {}", service.port))?;
    let process_info = process::inspect(pid).context("process exited or could not be inspected")?;
    if process_info.start_time != service.process_start_time {
        bail!("the process on this port has changed since it was scanned; scan again and retry");
    }
    check_kill_safety(pid, process_info.uid, &process_info.name)?;
    let pidfd =
        pidfd_open(pid).context("could not safely pin this process (pidfd is unavailable)")?;
    let still_owns_port = scanner::discover_services(config).iter().any(|current| {
        current.port == service.port
            && current.pid == Some(pid)
            && current.process_start_time == service.process_start_time
    });
    if !still_owns_port {
        #[cfg(target_os = "linux")]
        unsafe {
            libc::close(pidfd);
        }
        bail!("the service changed while preparing the signal; scan again and retry");
    }
    let signal = if force { libc::SIGKILL } else { libc::SIGTERM };
    pidfd_signal(pidfd, signal)
        .context("could not signal the service; it may have exited or belong to another user")?;
    if force {
        return Ok(format!("Force-stopped PID {pid}"));
    }

    let started = Instant::now();
    while started.elapsed() < Duration::from_millis(900) {
        if !Path::new(&format!("/proc/{pid}")).exists() {
            return Ok(format!("Stopped PID {pid} gracefully"));
        }
        thread::sleep(Duration::from_millis(30));
    }
    bail!("PID {pid} is still running after SIGTERM; review it and retry with --force if needed")
}

pub fn check_kill_safety(pid: u32, uid: Option<u32>, process_name: &str) -> anyhow::Result<()> {
    if pid <= 1 || pid == std::process::id() {
        bail!("refusing to signal protected PID {pid}");
    }
    let current_uid = unsafe { libc::geteuid() };
    if current_uid == 0 {
        bail!(
            "refusing to signal processes when PortPilot is running as root; run it as your desktop user"
        );
    }
    let uid = uid.context("could not verify the process owner")?;
    if uid != current_uid {
        bail!("refusing to signal a process owned by another user");
    }
    let normalized = process_name.to_ascii_lowercase();
    if [
        "systemd",
        "init",
        "kthreadd",
        "dbus-daemon",
        "dbus-broker",
        "sshd",
        "dockerd",
        "containerd",
        "cupsd",
        "systemd-resolve",
        "systemd-resolved",
        "networkmanager",
    ]
    .iter()
    .any(|name| normalized == *name)
    {
        bail!("refusing to signal critical system process '{process_name}'");
    }
    Ok(())
}

pub fn kill_summary(service: &Service) -> String {
    let mut lines = vec![
        service
            .project
            .as_ref()
            .map(|project| format!("Stop {}?", project.name))
            .unwrap_or_else(|| "Stop service?".into()),
    ];
    lines.push(format!("Port: {}", service.port));
    if let Some(docker) = &service.docker {
        lines.push(format!("Container: {}", docker.container));
        lines.push(format!("Image: {}", docker.image));
    } else {
        lines.push(format!(
            "PID: {}",
            service
                .pid
                .map_or_else(|| "unknown".into(), |pid| pid.to_string())
        ));
        lines.push(format!(
            "Command: {}",
            service.command.as_deref().unwrap_or(&service.process_name)
        ));
    }
    lines.join("\n")
}

fn find_program(configured: &str) -> Option<String> {
    let path = Path::new(configured);
    if path.components().count() > 1 {
        return path.is_file().then(|| configured.to_owned());
    }
    std::env::var_os("PATH")?
        .to_string_lossy()
        .split(':')
        .map(|directory| Path::new(directory).join(configured))
        .find(|candidate| candidate.is_file())
        .map(|candidate| candidate.to_string_lossy().into_owned())
}

fn pidfd_open(pid: u32) -> anyhow::Result<i32> {
    #[cfg(target_os = "linux")]
    {
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid as libc::pid_t, 0) as i32 };
        if fd < 0 {
            return Err(io::Error::last_os_error().into());
        }
        Ok(fd)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = pid;
        bail!("PortPilot process signaling requires Linux pidfd support")
    }
}

fn pidfd_signal(pidfd: i32, signal: i32) -> anyhow::Result<()> {
    #[cfg(target_os = "linux")]
    {
        let result = unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                pidfd,
                signal,
                std::ptr::null::<libc::siginfo_t>(),
                0,
            )
        };
        unsafe {
            libc::close(pidfd);
        }
        if result < 0 {
            return Err(io::Error::last_os_error().into());
        }
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (pidfd, signal);
        bail!("PortPilot process signaling requires Linux pidfd support")
    }
}

fn run_bounded(program: &str, args: &[&str], timeout: Duration) -> anyhow::Result<()> {
    let mut child = Command::new(program)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("could not run {program}; is it installed and available?"))?;
    let started = Instant::now();
    loop {
        match child.try_wait()? {
            Some(status) if status.success() => return Ok(()),
            Some(_) => bail!("{program} returned an error"),
            None if started.elapsed() < timeout => thread::sleep(Duration::from_millis(20)),
            None => {
                let _ = child.kill();
                let _ = child.wait();
                bail!("{program} timed out");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refuses_protected_pids() {
        assert!(check_kill_safety(1, Some(0), "systemd").is_err());
        assert!(check_kill_safety(std::process::id(), Some(1000), "node").is_err());
        assert!(check_kill_safety(42, Some(0), "node").is_err());
    }
    #[test]
    fn refuses_critical_process_names_even_for_user_processes() {
        assert!(check_kill_safety(42, Some(1000), "sshd").is_err());
    }
    #[test]
    fn allows_regular_unprivileged_project_process() {
        let uid = unsafe { libc::geteuid() };
        if uid == 0 {
            assert!(check_kill_safety(4242, Some(uid), "node").is_err());
        } else {
            assert!(check_kill_safety(4242, Some(uid), "node").is_ok());
        }
    }
    #[test]
    fn refuses_unknown_or_other_user_owners() {
        let uid = unsafe { libc::geteuid() };
        assert!(check_kill_safety(4242, None, "node").is_err());
        assert!(check_kill_safety(4242, Some(uid.saturating_add(1)), "node").is_err());
    }
}
