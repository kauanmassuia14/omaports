use crate::{models::DockerInfo, project};
use serde::Deserialize;
use std::{
    collections::HashMap,
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

#[derive(Deserialize)]
struct Container {
    #[serde(rename = "Id")]
    id: String,
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Config")]
    config: ContainerConfig,
    #[serde(rename = "NetworkSettings")]
    network: NetworkSettings,
    #[serde(rename = "Mounts", default)]
    mounts: Option<Vec<Mount>>,
}
#[derive(Deserialize)]
struct ContainerConfig {
    #[serde(rename = "Image")]
    image: String,
    #[serde(rename = "Labels", default)]
    labels: Option<HashMap<String, String>>,
}
#[derive(Deserialize)]
struct Mount {
    #[serde(rename = "Type", default)]
    kind: String,
    #[serde(rename = "Source", default)]
    source: String,
}
#[derive(Deserialize)]
struct NetworkSettings {
    #[serde(rename = "Ports", default)]
    ports: Option<HashMap<String, Option<Vec<PortBinding>>>>,
}
#[derive(Deserialize)]
struct PortBinding {
    #[serde(rename = "HostPort")]
    host_port: String,
}

pub fn published_ports() -> HashMap<u16, DockerInfo> {
    let Some(output) = run_docker(&["ps", "-q"]) else {
        return HashMap::new();
    };
    let ids_output = String::from_utf8_lossy(&output);
    let ids = ids_output
        .lines()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if ids.is_empty() {
        return HashMap::new();
    }
    let mut args = vec!["inspect", "--format", "{{json .}}"];
    args.extend(ids.iter().map(String::as_str));
    let Some(output) = run_docker(&args) else {
        return HashMap::new();
    };
    parse_inspect_output(&String::from_utf8_lossy(&output))
}

fn parse_inspect_output(output: &str) -> HashMap<u16, DockerInfo> {
    let mut found = HashMap::new();
    for line in output.lines() {
        let Ok(container) = serde_json::from_str::<Container>(line) else {
            continue;
        };
        for (container_binding, bindings) in container.network.ports.unwrap_or_default() {
            let Some(bindings) = bindings else { continue };
            let Some((port, _protocol)) = container_binding.split_once('/') else {
                continue;
            };
            let Ok(container_port) = port.parse::<u16>() else {
                continue;
            };
            for binding in bindings {
                let Ok(host_port) = binding.host_port.parse::<u16>() else {
                    continue;
                };
                let labels = container.config.labels.as_ref();
                let compose_project = labels
                    .and_then(|labels| labels.get("com.docker.compose.project"))
                    .cloned();
                let compose_service = labels
                    .and_then(|labels| labels.get("com.docker.compose.service"))
                    .cloned();
                let working_dir = labels
                    .and_then(|labels| labels.get("com.docker.compose.project.working_dir"))
                    .filter(|path| Path::new(path).is_dir())
                    .cloned()
                    .or_else(|| {
                        host_project_mount(container.mounts.as_deref().unwrap_or_default())
                    });
                let metadata = DockerInfo {
                    container: container.name.trim_start_matches('/').to_owned(),
                    container_id: container.id.chars().take(12).collect(),
                    image: container.config.image.clone(),
                    container_port,
                    compose_project,
                    compose_service,
                    working_dir,
                };
                found.entry(host_port).or_insert(metadata);
            }
        }
    }
    found
}

pub fn project_for_container(info: &DockerInfo, search_git_root: bool) -> Option<crate::Project> {
    info.working_dir
        .as_deref()
        .and_then(|path| project::resolve_project_at_with(Path::new(path), search_git_root))
        .or_else(|| {
            info.compose_project.as_ref().map(|name| crate::Project {
                name: name.clone(),
                root: info
                    .working_dir
                    .as_deref()
                    .filter(|path| Path::new(path).is_dir())
                    .unwrap_or_default()
                    .to_owned(),
                confidence: "medium".into(),
                source: "docker-compose-label".into(),
                git_branch: None,
            })
        })
}

fn host_project_mount(mounts: &[Mount]) -> Option<String> {
    mounts
        .iter()
        .filter(|mount| mount.kind == "bind" && !mount.source.is_empty())
        .filter_map(|mount| {
            project::resolve_project_at(Path::new(&mount.source)).map(|project| project.root)
        })
        .next()
}

fn run_docker(args: &[&str]) -> Option<Vec<u8>> {
    let mut child = Command::new("docker")
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let stdout = child.stdout.take()?;
    let reader = thread::spawn(move || {
        use std::io::Read;
        let mut bytes = Vec::new();
        let _ = std::io::BufReader::new(stdout)
            .take(8 * 1024 * 1024)
            .read_to_end(&mut bytes);
        bytes
    });
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success().then(|| reader.join().unwrap_or_default()),
            Ok(None) if started.elapsed() < Duration::from_millis(700) => {
                thread::sleep(Duration::from_millis(10))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                return None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compose_labels_can_identify_project_without_working_directory() {
        let info = DockerInfo {
            container: "postiq-db-1".into(),
            container_id: "123".into(),
            image: "postgres:16".into(),
            container_port: 5432,
            compose_project: Some("postiq".into()),
            compose_service: Some("db".into()),
            working_dir: None,
        };
        let project = project_for_container(&info, true).unwrap();
        assert_eq!(project.name, "postiq");
        assert_eq!(project.source, "docker-compose-label");
    }

    #[test]
    fn parses_published_port_and_compose_labels_from_inspect_json() {
        let output = r#"{"Id":"0123456789abcdef","Name":"/postiq-postgres-1","Config":{"Image":"postgres:16","WorkingDir":"/var/lib/postgresql","Labels":{"com.docker.compose.project":"postiq","com.docker.compose.service":"postgres","com.docker.compose.project.working_dir":"/home/km/Git/postiq"}},"NetworkSettings":{"Ports":{"5432/tcp":[{"HostIp":"127.0.0.1","HostPort":"15432"}]}}}"#;
        let found = parse_inspect_output(output);
        let service = found.get(&15432).unwrap();
        assert_eq!(service.container, "postiq-postgres-1");
        assert_eq!(service.container_port, 5432);
        assert_eq!(service.compose_project.as_deref(), Some("postiq"));
        assert_eq!(service.compose_service.as_deref(), Some("postgres"));
        assert_eq!(service.working_dir.as_deref(), Some("/home/km/Git/postiq"));
    }

    #[test]
    fn finds_a_project_in_a_host_bind_mount_without_compose_labels() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join(".git")).unwrap();
        let output = serde_json::json!({
            "Id": "abcdef0123456789",
            "Name": "/api",
            "Config": { "Image": "node:22", "Labels": {} },
            "NetworkSettings": { "Ports": { "3000/tcp": [{ "HostPort": "13000" }] } },
            "Mounts": [{ "Type": "bind", "Source": temp.path().to_string_lossy(), "Destination": "/app" }]
        });
        let found = parse_inspect_output(&output.to_string());
        assert_eq!(
            found.get(&13000).unwrap().working_dir.as_deref(),
            Some(temp.path().to_string_lossy().as_ref())
        );
    }

    #[test]
    fn docker_ports_survive_a_null_labels_field() {
        let output = r#"{"Id":"abcdef0123456789","Name":"/redis","Config":{"Image":"redis:7","Labels":null},"NetworkSettings":{"Ports":{"6379/tcp":[{"HostPort":"16379"}]}}}"#;
        let found = parse_inspect_output(output);
        assert_eq!(found.get(&16379).unwrap().container, "redis");
        assert!(found.get(&16379).unwrap().compose_project.is_none());
    }
}
