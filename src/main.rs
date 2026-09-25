use anyhow::Context;
use clap::{Parser, Subcommand};
use portpilot::{
    actions,
    config::Config,
    models::{Service, ServiceClass},
    scanner, ui, waybar,
};

#[derive(Parser)]
#[command(
    name = "portpilot",
    version,
    about = "Local development port manager for Linux and Omarchy",
    arg_required_else_help = false
)]
struct Cli {
    #[arg(long, global = true, help = "Enable diagnostic messages on stderr")]
    verbose: bool,
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// List development-classified TCP services (the default command)
    List {
        #[arg(long)]
        all: bool,
        #[arg(long)]
        json: bool,
    },
    /// Open the interactive service picker
    Ui,
    /// Print Waybar custom-module JSON
    Waybar,
    /// Show service details for a port
    Inspect { port: u16 },
    /// Open a detected HTTP service in the default browser
    Open { port: u16 },
    /// Print the detected project root for a port
    Project { port: u16 },
    /// Open a terminal in the detected project root
    Terminal { port: u16 },
    /// Open the detected project root in the configured editor
    Edit { port: u16 },
    /// Stop a process or Docker container listening on a port
    Kill {
        port: u16,
        #[arg(long)]
        force: bool,
        #[arg(long, short = 'y')]
        yes: bool,
        #[arg(long, hide = true)]
        expect_pid: Option<u32>,
        #[arg(long, hide = true)]
        expect_start_time: Option<u64>,
        #[arg(long, hide = true)]
        expect_name: Option<String>,
        #[arg(long, hide = true)]
        expect_container: Option<String>,
    },
}

fn main() {
    let cli = Cli::parse();
    let result = run(cli.command, cli.verbose);
    if let Err(error) = result {
        if cli.verbose {
            eprintln!("portpilot: {error:#}");
        } else {
            eprintln!("portpilot: {error}");
        }
        std::process::exit(1);
    }
}

fn run(command: Option<Commands>, verbose: bool) -> anyhow::Result<()> {
    let command = command.unwrap_or(Commands::List {
        all: false,
        json: false,
    });
    if matches!(&command, Commands::Waybar) {
        return run_waybar();
    }
    let config = Config::load()?;
    match command {
        Commands::List { all, json } => list(&config, all, json),
        Commands::Ui => ui::run(&config),
        Commands::Waybar => unreachable!(),
        Commands::Inspect { port } => {
            let service = actions::service_on_port(&config, port)?;
            ui::print_details(&service);
            Ok(())
        }
        Commands::Open { port } => {
            let service = actions::service_on_port(&config, port)?;
            actions::open_browser(&service)?;
            println!("Opened {}", service.url.as_deref().unwrap_or("service"));
            Ok(())
        }
        Commands::Project { port } => {
            let service = actions::service_on_port(&config, port)?;
            let project = service
                .project
                .as_ref()
                .with_context(|| format!("could not resolve a project for port {port}"))?;
            if project.root.is_empty() {
                anyhow::bail!(
                    "the project name is known, but no host working directory is available"
                );
            }
            println!("{}", project.root);
            Ok(())
        }
        Commands::Terminal { port } => {
            let service = actions::service_on_port(&config, port)?;
            actions::open_project(&config, &service, false)
        }
        Commands::Edit { port } => {
            let service = actions::service_on_port(&config, port)?;
            actions::open_project(&config, &service, true)
        }
        Commands::Kill {
            port,
            force,
            yes,
            expect_pid,
            expect_start_time,
            expect_name,
            expect_container,
        } => {
            let service = actions::service_on_port(&config, port)?;
            ensure_expected_service(
                &service,
                expect_pid,
                expect_start_time,
                expect_name.as_deref(),
                expect_container.as_deref(),
            )?;
            if !actions::confirm_kill(&service, force, yes)? {
                bail_cancelled();
                return Ok(());
            }
            println!("{}", actions::kill_service(&config, &service, force)?);
            Ok(())
        }
    }
    .map_err(|error| {
        if verbose {
            anyhow::anyhow!("{error:#}")
        } else {
            error
        }
    })
}

fn ensure_expected_service(
    service: &Service,
    pid: Option<u32>,
    start_time: Option<u64>,
    process_name: Option<&str>,
    container_id: Option<&str>,
) -> anyhow::Result<()> {
    if pid.is_some_and(|expected| service.pid != Some(expected))
        || start_time.is_some_and(|expected| service.process_start_time != Some(expected))
        || process_name.is_some_and(|expected| service.process_name != expected)
        || container_id.is_some_and(|expected| {
            service
                .docker
                .as_ref()
                .is_none_or(|docker| !docker.container_id.starts_with(expected))
        })
    {
        anyhow::bail!(
            "the selected service changed before the action started; refresh and select it again"
        );
    }
    Ok(())
}

fn list(config: &Config, all: bool, json: bool) -> anyhow::Result<()> {
    let services = scanner::discover_services_with(config, all)
        .into_iter()
        .filter(|service| all || service.class == ServiceClass::Development)
        .collect::<Vec<_>>();
    if json {
        println!("{}", serde_json::to_string_pretty(&services)?);
        return Ok(());
    }
    render_table(&services);
    Ok(())
}

fn render_table(services: &[Service]) {
    println!(
        "{:<7} {:<22} {:<18} {:<10} TYPE",
        "PORT", "PROJECT", "PROCESS", "PID"
    );
    println!("{:-<7} {:-<22} {:-<18} {:-<10} {:-<9}", "", "", "", "", "");
    for service in services {
        let project = service
            .project
            .as_ref()
            .map(|project| project.name.as_str())
            .or_else(|| {
                service
                    .docker
                    .as_ref()
                    .and_then(|docker| docker.compose_project.as_deref())
            })
            .unwrap_or("—");
        println!(
            "{:<7} {:<22} {:<18} {:<10} {}",
            service.port,
            truncate(project, 22),
            truncate(&service.process_name, 18),
            service
                .pid
                .map(|pid| pid.to_string())
                .unwrap_or_else(|| "—".into()),
            service.kind
        );
    }
    if services.is_empty() {
        println!("No listening services found.");
    }
}

fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        value.to_owned()
    } else {
        format!(
            "{}…",
            value
                .chars()
                .take(max.saturating_sub(1))
                .collect::<String>()
        )
    }
}

fn run_waybar() -> anyhow::Result<()> {
    // Waybar's stdout is a protocol channel: malformed optional config falls
    // back to defaults and every other diagnostic stays away from stdout.
    let config = Config::load().unwrap_or_default();
    println!("{}", waybar::current(&config.waybar.icon, &config));
    Ok(())
}

fn bail_cancelled() {
    eprintln!("portpilot: cancelled");
}

#[cfg(test)]
mod tests {
    use super::*;
    use portpilot::{DockerInfo, Project};

    fn process_service() -> Service {
        Service {
            port: 5173,
            protocol: "tcp".into(),
            bind_address: "127.0.0.1".into(),
            pid: Some(38291),
            process_start_time: Some(987654),
            process_name: "node".into(),
            command: Some("npm run dev".into()),
            cwd: Some("/home/km/Git/postiq".into()),
            user: Some("km".into()),
            project: Some(Project {
                name: "postiq".into(),
                root: "/home/km/Git/postiq".into(),
                confidence: "high".into(),
                source: "git-root".into(),
                git_branch: Some("main".into()),
            }),
            class: ServiceClass::Development,
            kind: "process".into(),
            url: Some("http://localhost:5173".into()),
            docker: None,
        }
    }

    #[test]
    fn ui_kill_guard_accepts_the_same_process_instance() {
        let service = process_service();
        assert!(
            ensure_expected_service(&service, Some(38291), Some(987654), Some("node"), None)
                .is_ok()
        );
    }

    #[test]
    fn ui_kill_guard_rejects_a_reused_port() {
        let service = process_service();
        assert!(
            ensure_expected_service(&service, Some(40000), Some(987654), Some("node"), None)
                .is_err()
        );
        assert!(
            ensure_expected_service(&service, Some(38291), Some(123), Some("node"), None).is_err()
        );
    }

    #[test]
    fn ui_kill_guard_matches_docker_container_identity() {
        let mut service = process_service();
        service.pid = None;
        service.process_start_time = None;
        service.docker = Some(DockerInfo {
            container: "postiq-db-1".into(),
            container_id: "0123456789ab".into(),
            image: "postgres:16".into(),
            container_port: 5432,
            compose_project: Some("postiq".into()),
            compose_service: Some("db".into()),
            working_dir: Some("/home/km/Git/postiq".into()),
        });
        assert!(ensure_expected_service(&service, None, None, None, Some("0123456789ab")).is_ok());
        assert!(ensure_expected_service(&service, None, None, None, Some("ffffffffffff")).is_err());
    }
}
