use crate::{actions, config::Config, models::Service, scanner};
use std::{
    io::{self, IsTerminal, Write},
    process::{Command, Stdio},
};

trait UiProvider {
    fn select(&self, title: &str, options: &[String]) -> anyhow::Result<Option<usize>>;
}

struct DmenuProvider {
    program: String,
}
struct TerminalProvider;

impl UiProvider for DmenuProvider {
    fn select(&self, title: &str, options: &[String]) -> anyhow::Result<Option<usize>> {
        let args = match self.program.as_str() {
            "fuzzel" => vec![
                "--dmenu".to_owned(),
                "--prompt".into(),
                format!("{title}> "),
            ],
            "rofi" => vec!["-dmenu".into(), "-p".into(), title.into()],
            _ => vec!["--dmenu".into(), "--prompt".into(), title.into()],
        };
        let mut child = Command::new(&self.program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        if let Some(mut stdin) = child.stdin.take() {
            for (index, option) in options.iter().enumerate() {
                writeln!(stdin, "{index}  {option}")?;
            }
        }
        let output = child.wait_with_output()?;
        if !output.status.success() {
            return Ok(None);
        }
        let selected = String::from_utf8_lossy(&output.stdout);
        let index = selected
            .split_whitespace()
            .next()
            .and_then(|value| value.parse::<usize>().ok());
        Ok(index.filter(|index| *index < options.len()))
    }
}

impl UiProvider for TerminalProvider {
    fn select(&self, title: &str, options: &[String]) -> anyhow::Result<Option<usize>> {
        println!("\n{title}");
        for (index, option) in options.iter().enumerate() {
            println!("  {}) {}", index + 1, option);
        }
        print!("Select (0 to cancel): ");
        io::stdout().flush()?;
        let mut answer = String::new();
        io::stdin().read_line(&mut answer)?;
        let selected = answer.trim().parse::<usize>().ok().unwrap_or(0);
        Ok((selected > 0 && selected <= options.len()).then_some(selected - 1))
    }
}

pub fn run(config: &Config) -> anyhow::Result<()> {
    if config.ui.provider == "auto" || config.ui.provider == "omarchy" {
        if open_omarchy_panel() {
            return Ok(());
        }
        if config.ui.provider == "omarchy" {
            anyhow::bail!(
                "Omarchy shell did not accept the PortPilot panel request; enable the plugin first"
            );
        }
    }
    let provider = choose_provider(&config.ui.provider)?;

    let services = scanner::discover_services(config)
        .into_iter()
        .filter(|service| service.class == crate::models::ServiceClass::Development)
        .collect::<Vec<_>>();
    if services.is_empty() {
        println!("No local development services found.");
        return Ok(());
    }
    let service_options = services.iter().map(service_label).collect::<Vec<_>>();
    let Some(index) = provider.select("PortPilot", &service_options)? else {
        return Ok(());
    };
    let service = &services[index];
    let mut actions_menu = vec!["Show details".to_owned()];
    if service.url.is_some() {
        actions_menu.push("Open in browser".into());
    }
    if service
        .project
        .as_ref()
        .is_some_and(|project| !project.root.is_empty())
    {
        actions_menu.push("Open terminal".into());
        actions_menu.push("Open editor".into());
    }
    actions_menu.push("Stop service".into());
    let Some(action) = provider.select(
        &format!("{} · {}", service.port, service_label(service)),
        &actions_menu,
    )?
    else {
        return Ok(());
    };
    match actions_menu[action].as_str() {
        "Show details" => print_details(service),
        "Open in browser" => actions::open_browser(service)?,
        "Open terminal" => actions::open_project(config, service, false)?,
        "Open editor" => actions::open_project(config, service, true)?,
        "Stop service" => {
            print_details(service);
            let Some(confirmed) = provider.select(
                "Stop this service?",
                &["Cancel".into(), "Stop with SIGTERM".into()],
            )?
            else {
                return Ok(());
            };
            if confirmed == 1 {
                println!("{}", actions::kill_service(config, service, false)?);
            }
        }
        _ => unreachable!("only listed actions can be selected"),
    }
    Ok(())
}

fn choose_provider(setting: &str) -> anyhow::Result<Box<dyn UiProvider>> {
    if setting != "auto" && setting != "omarchy" && setting != "terminal" {
        if executable_in_path(setting) {
            return Ok(Box::new(DmenuProvider {
                program: setting.into(),
            }));
        }
        anyhow::bail!("configured PortPilot UI provider '{setting}' was not found in PATH");
    }
    if setting != "terminal" {
        for program in ["fuzzel", "rofi", "wofi"] {
            if executable_in_path(program) {
                return Ok(Box::new(DmenuProvider {
                    program: program.into(),
                }));
            }
        }
    }
    if io::stdin().is_terminal() && io::stdout().is_terminal() {
        Ok(Box::new(TerminalProvider))
    } else {
        anyhow::bail!(
            "no menu provider found; install fuzzel or rofi-wayland, or run from a terminal"
        )
    }
}

fn open_omarchy_panel() -> bool {
    Command::new("omarchy-shell")
        .args([
            "shell",
            "toggle",
            "io.github.kauanmassuia14.portpilot",
            "{}",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn executable_in_path(program: &str) -> bool {
    if program.contains('/') {
        return std::path::Path::new(program).is_file();
    }
    std::env::var_os("PATH").is_some_and(|path| {
        path.to_string_lossy()
            .split(':')
            .any(|dir| std::path::Path::new(dir).join(program).is_file())
    })
}

pub fn service_label(service: &Service) -> String {
    let project = service
        .project
        .as_ref()
        .map(|project| project.name.as_str())
        .unwrap_or("unknown project");
    format!("{}  {} — {}", service.port, project, service.process_name)
}

pub fn print_details(service: &Service) {
    println!("Service");
    println!(
        "  Project     {}",
        service
            .project
            .as_ref()
            .map(|project| project.name.as_str())
            .unwrap_or("unknown")
    );
    println!("  Port        {}", service.port);
    if let Some(url) = &service.url {
        println!("  URL         {url}");
    }
    println!("  Process     {}", service.process_name);
    if let Some(pid) = service.pid {
        println!("  PID         {pid}");
    }
    if let Some(command) = &service.command {
        println!("  Command     {command}");
    }
    if let Some(root) = service
        .project
        .as_ref()
        .map(|project| project.root.as_str())
        .filter(|root| !root.is_empty())
        .or(service.cwd.as_deref())
    {
        println!("  Root        {}", display_path(root));
    }
    if let Some(branch) = service
        .project
        .as_ref()
        .and_then(|project| project.git_branch.as_deref())
    {
        println!("  Git         {branch}");
    }
    if let Some(docker) = &service.docker {
        println!("  Container   {}", docker.container);
        println!(
            "  Service     {}",
            docker.compose_service.as_deref().unwrap_or("—")
        );
        println!("  Image       {}", docker.image);
        println!("  Type        Docker");
    } else {
        println!("  Type        Local Process");
        if let Some(user) = &service.user {
            println!("  User        {user}");
        }
    }
}

fn display_path(path: &str) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    path.strip_prefix(&home)
        .map(|rest| format!("~{rest}"))
        .unwrap_or_else(|| path.to_owned())
}
