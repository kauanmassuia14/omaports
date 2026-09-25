use crate::{
    models::{Service, ServiceClass},
    process::ProcessInfo,
};

pub fn classify(
    process: Option<&ProcessInfo>,
    project_found: bool,
    docker_compose: bool,
) -> ServiceClass {
    if project_found || docker_compose {
        return ServiceClass::Development;
    }
    let Some(process) = process else {
        return ServiceClass::Unknown;
    };
    let name = process.name.to_ascii_lowercase();
    let command = process
        .command
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let looks_like_app_server = [
        "node", "npm", "pnpm", "yarn", "bun", "python", "uvicorn", "gunicorn", "ruby", "rails",
        "php", "artisan", "cargo", "go", "java", "vite", "webpack", "next", "deno",
    ]
    .iter()
    .any(|part| name.contains(part) || command.contains(part));
    if looks_like_app_server {
        return ServiceClass::Development;
    }
    if process.systemd_service
        || process.uid == Some(0)
        || [
            "systemd-resolve",
            "systemd-resolved",
            "cupsd",
            "sshd",
            "avahi-daemon",
        ]
        .iter()
        .any(|part| name.contains(part))
    {
        return ServiceClass::System;
    }
    ServiceClass::Unknown
}

pub fn is_development(service: &Service) -> bool {
    service.class == ServiceClass::Development
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_project_is_development_regardless_of_port() {
        let process = ProcessInfo {
            uid: Some(1000),
            ..ProcessInfo::default()
        };
        assert_eq!(
            classify(Some(&process), true, false),
            ServiceClass::Development
        );
    }
    #[test]
    fn root_owned_daemon_is_system_when_no_project_is_known() {
        let process = ProcessInfo {
            uid: Some(0),
            name: "sshd".into(),
            ..ProcessInfo::default()
        };
        assert_eq!(classify(Some(&process), false, false), ServiceClass::System);
    }
    #[test]
    fn project_compose_service_is_development() {
        assert_eq!(classify(None, false, true), ServiceClass::Development);
    }

    #[test]
    fn rootless_systemd_unit_without_project_is_system() {
        let process = ProcessInfo {
            uid: Some(1000),
            name: "postgres".into(),
            systemd_service: true,
            ..ProcessInfo::default()
        };
        assert_eq!(classify(Some(&process), false, false), ServiceClass::System);
    }
}
