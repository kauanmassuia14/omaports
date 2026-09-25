use crate::models::{Service, ServiceClass};
use serde::Serialize;

#[derive(Serialize)]
struct WaybarOutput<'a> {
    text: String,
    tooltip: &'a str,
    class: &'a str,
}

pub fn render(services: &[Service], icon: &str) -> String {
    let count = services
        .iter()
        .filter(|service| service.class == ServiceClass::Development)
        .count();
    let mut projects = services
        .iter()
        .filter(|service| service.class == ServiceClass::Development)
        .map(|service| {
            format!(
                "{}  {}",
                service.port,
                service
                    .project
                    .as_ref()
                    .map(|project| project.name.as_str())
                    .unwrap_or(&service.process_name)
            )
        })
        .collect::<Vec<_>>();
    projects.sort();
    projects.dedup();
    let tooltip = if count == 0 {
        "No local development services".to_owned()
    } else {
        format!(
            "{count} local development service{}\n{}",
            if count == 1 { "" } else { "s" },
            projects.join("\n")
        )
    };
    serde_json::to_string(&WaybarOutput {
        text: format!("{icon} {count}"),
        tooltip: &tooltip,
        class: if count == 0 { "idle" } else { "active" },
    })
    .unwrap_or_else(|_| {
        r#"{"text":"PortPilot","tooltip":"Status unavailable","class":"idle"}"#.into()
    })
}

pub fn current(icon: &str, config: &crate::config::Config) -> String {
    render(&crate::cache::services_for_waybar(config), icon)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        models::ServiceClass,
        models::{Project, Service},
    };

    fn service(port: u16, project: &str, class: ServiceClass) -> Service {
        Service {
            port,
            protocol: "tcp".into(),
            bind_address: "127.0.0.1".into(),
            pid: Some(1),
            process_start_time: Some(42),
            process_name: "node".into(),
            command: None,
            cwd: None,
            user: None,
            project: Some(Project {
                name: project.into(),
                root: "/tmp".into(),
                confidence: "high".into(),
                source: "git-root".into(),
                git_branch: None,
            }),
            class,
            kind: "process".into(),
            url: None,
            docker: None,
        }
    }
    #[test]
    fn json_has_waybar_contract_and_counts_development_only() {
        let rendered = render(
            &[
                service(5173, "postiq", ServiceClass::Development),
                service(53, "dns", ServiceClass::System),
            ],
            "PP",
        );
        let json: serde_json::Value = serde_json::from_str(&rendered).unwrap();
        assert_eq!(json["text"], "PP 1");
        assert_eq!(json["class"], "active");
        assert!(json["tooltip"].as_str().unwrap().contains("postiq"));
    }
    #[test]
    fn idle_state_is_valid_json() {
        let json: serde_json::Value = serde_json::from_str(&render(&[], "PP")).unwrap();
        assert_eq!(json["text"], "PP 0");
        assert_eq!(json["class"], "idle");
    }
}
