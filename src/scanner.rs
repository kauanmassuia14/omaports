use crate::{classify, config::Config, docker, models::Service, process, project};
use std::{
    collections::{HashMap, HashSet},
    fs,
    net::{Ipv4Addr, Ipv6Addr},
    path::Path,
};

#[derive(Clone, Debug)]
pub struct Listener {
    pub port: u16,
    pub protocol: &'static str,
    pub address: String,
    pub inode: String,
}

pub fn scan() -> Vec<Listener> {
    let mut listeners = Vec::new();
    for (path, protocol, ipv6) in [
        ("/proc/net/tcp", "tcp", false),
        ("/proc/net/tcp6", "tcp", true),
    ] {
        let Ok(contents) = fs::read_to_string(path) else {
            continue;
        };
        listeners.extend(parse_table(&contents, protocol, ipv6));
    }
    listeners.sort_by_key(|listener| listener.port);
    listeners
}

pub fn discover_services(config: &Config) -> Vec<Service> {
    discover_services_with(config, false)
}

pub fn discover_services_with(config: &Config, show_ignored_ports: bool) -> Vec<Service> {
    let listeners = scan();
    let owners = socket_owners();
    let containers = docker::published_ports();
    let mut services = Vec::new();
    let mut docker_ports = HashSet::new();

    for (port, metadata) in &containers {
        if !show_ignored_ports && config.ports.ignore.contains(port) {
            continue;
        }
        docker_ports.insert(*port);
        let project = docker::project_for_container(metadata, config.projects.search_git_root);
        let has_project = project.is_some();
        services.push(Service {
            port: *port,
            protocol: "tcp".into(),
            bind_address: "0.0.0.0".into(),
            pid: None,
            process_start_time: None,
            process_name: metadata.compose_service.clone().unwrap_or_else(|| {
                metadata
                    .image
                    .split(':')
                    .next()
                    .unwrap_or("container")
                    .to_owned()
            }),
            command: None,
            cwd: metadata.working_dir.clone(),
            user: None,
            project,
            class: classify::classify(None, has_project, metadata.compose_project.is_some()),
            kind: "docker".into(),
            url: local_url(
                *port,
                metadata.container_port,
                &metadata.image,
                metadata.compose_service.as_deref(),
            ),
            docker: Some(metadata.clone()),
        });
    }

    let mut seen = HashSet::new();
    for listener in listeners {
        if (!show_ignored_ports && config.ports.ignore.contains(&listener.port))
            || docker_ports.contains(&listener.port)
        {
            continue;
        }
        let pid = owners.get(&listener.inode).copied();
        if !seen.insert((listener.port, pid)) {
            continue;
        }
        let process_info = pid.and_then(process::inspect);
        let resolved_project = process_info
            .as_ref()
            .and_then(|info| project::resolve_project_with(info, config.projects.search_git_root));
        let class = classify::classify(process_info.as_ref(), resolved_project.is_some(), false);
        let process_name = process_info
            .as_ref()
            .map(|info| info.name.clone())
            .unwrap_or_else(|| "unknown".into());
        let command = process_info.as_ref().and_then(|info| info.command.clone());
        let cwd = process_info.as_ref().and_then(|info| {
            info.cwd
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned())
        });
        let user = process_info
            .as_ref()
            .and_then(|info| info.uid)
            .and_then(process::user_name);
        let url = local_url(
            listener.port,
            listener.port,
            &process_name,
            command.as_deref(),
        );
        services.push(Service {
            port: listener.port,
            protocol: listener.protocol.into(),
            bind_address: listener.address,
            pid,
            process_start_time: process_info.as_ref().and_then(|info| info.start_time),
            process_name,
            command,
            cwd,
            user,
            project: resolved_project,
            class,
            kind: "process".into(),
            url,
            docker: None,
        });
    }
    services.sort_by(|left, right| {
        left.port
            .cmp(&right.port)
            .then(left.process_name.cmp(&right.process_name))
    });
    services
}

pub fn is_http(port: u16, process_name: &str, command: Option<&str>) -> bool {
    const COMMON_HTTP_PORTS: [u16; 18] = [
        80, 443, 3000, 3001, 4000, 4200, 5000, 5173, 8000, 8001, 8080, 8081, 8443, 8888, 9000,
        9001, 4173, 5001,
    ];
    if COMMON_HTTP_PORTS.contains(&port) {
        return true;
    }
    let process = process_name.to_ascii_lowercase();
    let command = command.unwrap_or_default().to_ascii_lowercase();
    [
        "http.server",
        "uvicorn",
        "gunicorn",
        "next",
        "vite",
        "webpack-dev-server",
        "rails server",
        "django",
    ]
    .iter()
    .any(|hint| process.contains(hint) || command.contains(hint))
}

pub fn local_url(
    host_port: u16,
    service_port: u16,
    process_name: &str,
    command: Option<&str>,
) -> Option<String> {
    if !is_http(service_port, process_name, command) {
        return None;
    }
    let scheme = if [443, 8443].contains(&service_port) {
        "https"
    } else {
        "http"
    };
    Some(format!("{scheme}://localhost:{host_port}"))
}

pub fn socket_owners() -> HashMap<String, u32> {
    let mut owners = HashMap::new();
    let Ok(entries) = fs::read_dir("/proc") else {
        return owners;
    };
    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        let Ok(fds) = fs::read_dir(entry.path().join("fd")) else {
            continue;
        };
        for fd in fds.flatten() {
            let Ok(target) = fs::read_link(fd.path()) else {
                continue;
            };
            if let Some(inode) = socket_inode(&target) {
                owners.entry(inode).or_insert(pid);
            }
        }
    }
    owners
}

fn parse_table(contents: &str, protocol: &'static str, ipv6: bool) -> Vec<Listener> {
    contents
        .lines()
        .skip(1)
        .filter_map(|line| {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            if fields.len() < 10 || fields[3] != "0A" {
                return None;
            }
            let (address, port) = parse_local(fields[1], ipv6)?;
            Some(Listener {
                port,
                protocol,
                address,
                inode: fields[9].to_owned(),
            })
        })
        .collect()
}

fn parse_local(value: &str, ipv6: bool) -> Option<(String, u16)> {
    let (address_hex, port_hex) = value.split_once(':')?;
    let port = u16::from_str_radix(port_hex, 16).ok()?;
    if ipv6 {
        if address_hex.len() != 32 {
            return None;
        }
        let mut bytes = [0u8; 16];
        let (words, remainder) = address_hex.as_bytes().as_chunks::<8>();
        if !remainder.is_empty() {
            return None;
        }
        for (index, word) in words.iter().enumerate() {
            let text = std::str::from_utf8(word).ok()?;
            let native = u32::from_str_radix(text, 16).ok()?.to_le_bytes();
            bytes[index * 4..index * 4 + 4].copy_from_slice(&native);
        }
        Some((Ipv6Addr::from(bytes).to_string(), port))
    } else {
        let number = u32::from_str_radix(address_hex, 16).ok()?;
        Some((Ipv4Addr::from(number.to_le_bytes()).to_string(), port))
    }
}

fn socket_inode(path: &Path) -> Option<String> {
    let target = path.to_string_lossy();
    let inner = target.strip_prefix("socket:[")?.strip_suffix(']')?;
    Some(inner.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ipv4_listener() {
        let table = "sl local_address rem_address st tx_queue:rx_queue tr:tm->when retrnsmt uid timeout inode\n  0: 0100007F:1454 00000000:0000 0A 00000000:00000000 00:00000000 00000000 1000 0 12345";
        let found = parse_table(table, "tcp", false);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].address, "127.0.0.1");
        assert_eq!(found[0].port, 5204);
        assert_eq!(found[0].inode, "12345");
    }

    #[test]
    fn ignores_non_listening_rows() {
        let table = "header\n  0: 0100007F:1 00000000:0000 01 00:00 00:00 0 1000 0 10";
        assert!(parse_table(table, "tcp", false).is_empty());
    }

    #[test]
    fn parses_ipv6_listener() {
        let table = "header\n  0: 00000000000000000000000001000000:1454 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000 1000 0 12345";
        let found = parse_table(table, "tcp", true);
        assert_eq!(found[0].address, "::1");
        assert_eq!(found[0].port, 5204);
    }

    #[test]
    fn guesses_http_urls_without_treating_database_ports_as_web_services() {
        assert_eq!(
            local_url(5173, 5173, "node", None).as_deref(),
            Some("http://localhost:5173")
        );
        assert_eq!(
            local_url(8443, 8443, "nginx", None).as_deref(),
            Some("https://localhost:8443")
        );
        assert_eq!(local_url(15432, 5432, "postgres", None), None);
        assert_eq!(
            local_url(18080, 8080, "nginx", None).as_deref(),
            Some("http://localhost:18080")
        );
    }
}
