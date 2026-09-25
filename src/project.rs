use crate::{models::Project, process::ProcessInfo};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub fn resolve_project(process: &ProcessInfo) -> Option<Project> {
    resolve_project_with(process, true)
}

pub fn resolve_project_with(process: &ProcessInfo, search_git_root: bool) -> Option<Project> {
    process
        .cwd
        .as_deref()
        .and_then(|path| resolve_project_at_with(path, search_git_root))
        .or_else(|| {
            process.ancestors.iter().find_map(|ancestor| {
                ancestor
                    .cwd
                    .as_deref()
                    .and_then(|path| resolve_project_at_with(path, search_git_root))
            })
        })
        .or_else(|| {
            process
                .command
                .as_deref()
                .and_then(|command| project_from_command(command, search_git_root))
        })
        .or_else(|| {
            process.ancestors.iter().find_map(|ancestor| {
                ancestor
                    .command
                    .as_deref()
                    .and_then(|command| project_from_command(command, search_git_root))
            })
        })
}

pub fn resolve_project_at(path: &Path) -> Option<Project> {
    resolve_project_at_with(path, true)
}

pub fn resolve_project_at_with(path: &Path, search_git_root: bool) -> Option<Project> {
    let mut start = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()?.to_path_buf()
    };
    if !start.is_absolute() {
        return None;
    }

    let mut package_candidate: Option<Project> = None;
    loop {
        if search_git_root && is_git_root(&start) {
            let mut project = make_project(&start, "git-root", "high");
            if let Some(package) = package_candidate {
                project.name = package.name;
            }
            return Some(project);
        }
        if has_project_manifest(&start) && package_candidate.is_none() {
            package_candidate = Some(make_project(&start, manifest_source(&start), "medium"));
        }
        if !start.pop() {
            break;
        }
    }
    package_candidate
}

fn project_from_command(command: &str, search_git_root: bool) -> Option<Project> {
    command.split_whitespace().find_map(|token| {
        let clean =
            token.trim_matches(|character: char| matches!(character, '\'' | '"' | ',' | ';'));
        if clean.starts_with('/') && Path::new(clean).exists() {
            resolve_project_at_with(Path::new(clean), search_git_root)
        } else {
            None
        }
    })
}

fn is_git_root(path: &Path) -> bool {
    path.join(".git").is_dir() || path.join(".git").is_file()
}

fn has_project_manifest(path: &Path) -> bool {
    [
        "package.json",
        "pyproject.toml",
        "Cargo.toml",
        "go.mod",
        "compose.yml",
        "compose.yaml",
        "docker-compose.yml",
        "docker-compose.yaml",
    ]
    .iter()
    .any(|name| path.join(name).is_file())
}

fn manifest_source(path: &Path) -> &'static str {
    if path.join("package.json").is_file() {
        "package.json"
    } else if path.join("pyproject.toml").is_file() {
        "pyproject.toml"
    } else if path.join("Cargo.toml").is_file() {
        "Cargo.toml"
    } else if path.join("go.mod").is_file() {
        "go.mod"
    } else {
        "compose-file"
    }
}

fn make_project(root: &Path, source: &str, confidence: &str) -> Project {
    let root = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let name = project_name(&root).unwrap_or_else(|| {
        root.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("project")
            .to_owned()
    });
    Project {
        name,
        root: root.to_string_lossy().into_owned(),
        confidence: confidence.to_owned(),
        source: source.to_owned(),
        git_branch: git_branch(&root),
    }
}

fn project_name(root: &Path) -> Option<String> {
    let package = root.join("package.json");
    if let Ok(contents) = fs::read_to_string(package)
        && let Ok(value) = serde_json::from_str::<serde_json::Value>(&contents)
        && let Some(name) = value.get("name").and_then(serde_json::Value::as_str)
    {
        return Some(name.to_owned());
    }
    for manifest in ["pyproject.toml", "Cargo.toml"] {
        let Ok(contents) = fs::read_to_string(root.join(manifest)) else {
            continue;
        };
        let Ok(parsed) = toml::from_str::<toml::Value>(&contents) else {
            continue;
        };
        let name = if manifest == "pyproject.toml" {
            parsed.get("project").and_then(|table| table.get("name"))
        } else {
            parsed.get("package").and_then(|table| table.get("name"))
        }
        .and_then(toml::Value::as_str);
        if let Some(name) = name {
            return Some(name.to_owned());
        }
    }
    if let Ok(contents) = fs::read_to_string(root.join("go.mod"))
        && let Some(module) = contents
            .lines()
            .find_map(|line| line.strip_prefix("module "))
    {
        return module.split('/').next_back().map(str::to_owned);
    }
    None
}

fn git_branch(root: &Path) -> Option<String> {
    let git_dir = fs::read_to_string(root.join(".git"))
        .ok()
        .and_then(|contents| {
            contents.trim().strip_prefix("gitdir: ").map(|path| {
                let path = PathBuf::from(path);
                if path.is_absolute() {
                    path
                } else {
                    root.join(path)
                }
            })
        })
        .unwrap_or_else(|| root.join(".git"));
    let head = fs::read_to_string(git_dir.join("HEAD")).ok()?;
    head.trim()
        .strip_prefix("ref: refs/heads/")
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::ProcessSnapshot;
    use tempfile::tempdir;

    #[test]
    fn finds_git_root_and_package_name_from_nested_cwd() {
        let temp = tempdir().unwrap();
        let root = temp.path().join("postiq");
        fs::create_dir_all(root.join(".git/refs/heads")).unwrap();
        fs::create_dir_all(root.join("apps/web/node_modules/.bin")).unwrap();
        fs::write(root.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        fs::write(root.join("package.json"), r#"{"name":"postiq"}"#).unwrap();
        let process = ProcessInfo {
            pid: 42,
            name: "node".into(),
            command: Some("node vite".into()),
            cwd: Some(root.join("apps/web")),
            uid: Some(1000),
            start_time: Some(42),
            systemd_service: false,
            ancestors: vec![ProcessSnapshot {
                pid: 41,
                name: "npm".into(),
                command: None,
                cwd: None,
                start_time: None,
            }],
        };
        let project = resolve_project(&process).unwrap();
        assert_eq!(project.name, "postiq");
        assert_eq!(project.source, "git-root");
        assert_eq!(project.git_branch.as_deref(), Some("main"));
    }

    #[test]
    fn package_manifest_resolves_without_git() {
        let temp = tempdir().unwrap();
        fs::write(temp.path().join("package.json"), r#"{"name":"vite-app"}"#).unwrap();
        let project = resolve_project_at(temp.path()).unwrap();
        assert_eq!(project.name, "vite-app");
        assert_eq!(project.source, "package.json");
    }

    #[test]
    fn cargo_manifest_name_is_used() {
        let temp = tempdir().unwrap();
        fs::write(
            temp.path().join("Cargo.toml"),
            "[package]\nname = \"pilot-core\"\n",
        )
        .unwrap();
        let parsed: toml::Value =
            toml::from_str(&fs::read_to_string(temp.path().join("Cargo.toml")).unwrap()).unwrap();
        assert_eq!(
            parsed
                .get("package")
                .and_then(|table| table.get("name"))
                .and_then(toml::Value::as_str),
            Some("pilot-core")
        );
        assert_eq!(resolve_project_at(temp.path()).unwrap().name, "pilot-core");
    }

    #[test]
    fn parent_process_cwd_can_supply_the_project_root() {
        let temp = tempdir().unwrap();
        let root = temp.path().join("postiq-api");
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join("package.json"), r#"{"name":"postiq-api"}"#).unwrap();
        let process = ProcessInfo {
            pid: 99,
            name: "node".into(),
            cwd: None,
            ancestors: vec![ProcessSnapshot {
                pid: 98,
                name: "npm".into(),
                command: Some("npm run dev".into()),
                cwd: Some(root.clone()),
                start_time: None,
            }],
            ..ProcessInfo::default()
        };
        let project = resolve_project(&process).unwrap();
        assert_eq!(project.name, "postiq-api");
        assert_eq!(project.root, root.to_string_lossy());
    }

    #[test]
    fn git_search_can_be_disabled() {
        let temp = tempdir().unwrap();
        fs::create_dir_all(temp.path().join(".git")).unwrap();
        fs::write(
            temp.path().join("package.json"),
            r#"{"name":"manifest-name"}"#,
        )
        .unwrap();
        let project = resolve_project_at_with(temp.path(), false).unwrap();
        assert_eq!(project.source, "package.json");
        assert_eq!(project.confidence, "medium");
    }

    #[test]
    fn command_line_path_can_supply_a_project_when_cwd_is_unavailable() {
        let temp = tempdir().unwrap();
        let root = temp.path().join("api-service");
        fs::create_dir_all(root.join(".git")).unwrap();
        let executable = root.join("bin/server");
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        fs::write(&executable, "fixture").unwrap();
        let process = ProcessInfo {
            name: "node".into(),
            command: Some(format!("node {}", executable.display())),
            cwd: None,
            ..ProcessInfo::default()
        };
        let project = resolve_project(&process).unwrap();
        assert_eq!(project.root, root.to_string_lossy());
    }

    #[test]
    fn reads_python_project_name_from_pyproject_toml() {
        let temp = tempdir().unwrap();
        fs::write(
            temp.path().join("pyproject.toml"),
            "[project]\nname = \"api-service\"\n",
        )
        .unwrap();
        let project = resolve_project_at(temp.path()).unwrap();
        assert_eq!(project.name, "api-service");
        assert_eq!(project.source, "pyproject.toml");
    }
}
