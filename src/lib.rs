pub mod actions;
pub mod cache;
pub mod classify;
pub mod config;
pub mod docker;
pub mod process;
pub mod project;
pub mod scanner;
pub mod ui;
pub mod waybar;

pub use models::{DockerInfo, Project, Service, ServiceClass};

pub mod models {
    use serde::{Deserialize, Serialize};

    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
    pub struct Project {
        pub name: String,
        pub root: String,
        pub confidence: String,
        pub source: String,
        pub git_branch: Option<String>,
    }

    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
    pub struct DockerInfo {
        pub container: String,
        pub container_id: String,
        pub image: String,
        pub container_port: u16,
        pub compose_project: Option<String>,
        pub compose_service: Option<String>,
        pub working_dir: Option<String>,
    }

    #[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
    #[serde(rename_all = "lowercase")]
    pub enum ServiceClass {
        Development,
        System,
        Unknown,
    }

    impl ServiceClass {
        pub fn as_str(self) -> &'static str {
            match self {
                Self::Development => "development",
                Self::System => "system",
                Self::Unknown => "unknown",
            }
        }
    }

    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
    pub struct Service {
        pub port: u16,
        pub protocol: String,
        pub bind_address: String,
        pub pid: Option<u32>,
        pub process_start_time: Option<u64>,
        pub process_name: String,
        pub command: Option<String>,
        pub cwd: Option<String>,
        pub user: Option<String>,
        pub project: Option<Project>,
        pub class: ServiceClass,
        pub kind: String,
        pub url: Option<String>,
        pub docker: Option<DockerInfo>,
    }
}
