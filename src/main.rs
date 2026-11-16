#![allow(clippy::collapsible_if, clippy::collapsible_else_if)]

use ccp::daemon;
use ccp::probe::{self, ProbeResult};
use ccp::state::DaemonState;
use ccp::wifi;
use std::env;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();

    // 1. Save credentials: `ccp set`
    if args.len() > 1 && args[1] == "set" {
        let mut username = String::new();

        print!("Username: ");
        io::stdout().flush()?;
        io::stdin().read_line(&mut username)?;

        let username = username.trim();
        let password = rpassword::prompt_password("Password: ")?;

        if username.is_empty() || password.is_empty() {
            eprintln!("Username and password cannot be empty.");
            return Err("Username and password cannot be empty.".into());
        }

        let mut file = create_credentials_file()?;

        // Store credentials as raw text: username on line 1, password on line 2.
        writeln!(file, "{}", username)?;
        writeln!(file, "{}", password)?;

        // Reset daemon circuit breaker on new credentials
        let state_path = get_config_dir().join("state.json");
        DaemonState::reset_circuit_breaker(&state_path);

        println!("Credentials saved successfully.");
        return Ok(());
    }

    // 2. Start background daemon: `ccp start`
    if args.len() > 1 && args[1] == "start" {
        if let Err(e) = load_credentials() {
            eprintln!("{e}");
            return Err(e.into());
        }
        let config_dir = get_config_dir();
        if let Err(e) = daemon::start_background_daemon(&config_dir) {
            eprintln!("[ERROR] {e}");
            return Err(e.into());
        }
        return Ok(());
    }

    // 3. Stop background daemon: `ccp stop`
    if args.len() > 1 && args[1] == "stop" {
        let config_dir = get_config_dir();
        if let Err(e) = daemon::stop_daemon(&config_dir) {
            eprintln!("[ERROR] {e}");
            return Err(e.into());
        }
        return Ok(());
    }

    // 4. Print status: `ccp status`
    if args.len() > 1 && args[1] == "status" {
        let config_dir = get_config_dir();
        daemon::print_status(&config_dir);
        return Ok(());
    }

    // 5. Run daemon loop: `ccp daemon [--once] [--dry-run]`
    if args.len() > 1 && args[1] == "daemon" {
        let (username, password) = match load_credentials() {
            Ok(creds) => creds,
            Err(e) => {
                eprintln!("{e}");
                return Err(e.into());
            }
        };

        let once = args.iter().any(|arg| arg == "--once");
        let dry_run = args.iter().any(|arg| arg == "--dry-run");

        let config_dir = get_config_dir();
        if let Err(e) = daemon::run_daemon(&config_dir, &username, &password, once, dry_run) {
            eprintln!("[ERROR] Daemon error: {e}");
            return Err(e.into());
        }
        return Ok(());
    }

    // 6. Autostart configuration: `ccp autostart [enable [--now]|disable]`
    if args.len() > 1 && args[1] == "autostart" {
        let config_dir = get_config_dir();
        if args.len() > 2 && args[2] == "enable" {
            let start_now = args.iter().any(|a| a == "--now" || a == "-n");
            if let Err(e) = daemon::set_autostart(&config_dir, true) {
                eprintln!("[ERROR] {e}");
                return Err(e.into());
            }

            if start_now {
                if let Err(e) = daemon::start_background_daemon(&config_dir) {
                    eprintln!("[ERROR] Failed to start background daemon: {e}");
                    return Err(e.into());
                }
            } else {
                println!("Auto-start configured. The daemon will automatically launch on your next system restart/login.");
                println!("Tip: Run 'ccp start' or use 'ccp autostart enable --now' to start the daemon immediately.");
            }
        } else if args.len() > 2 && args[2] == "disable" {
            if let Err(e) = daemon::set_autostart(&config_dir, false) {
                eprintln!("[ERROR] {e}");
                return Err(e.into());
            }
        } else {
            eprintln!("Usage: ccp autostart [enable [--now]|disable]");
            return Err("Invalid autostart arguments.".into());
        }
        return Ok(());
    }

    // Default (args.len() == 1): load saved credentials and authenticate immediately
    if args.len() == 1 {
        let (username, password) = match load_credentials() {
            Ok(creds) => creds,
            Err(e) => {
                eprintln!("{e}");
                return Err(e.into());
            }
        };

        let cached_gateway = get_cached_gateway();
        let client = probe::build_fast_client(false)?;

        // Execute zero-lag probe pipeline
        let probe_result = probe::probe_network(&client, cached_gateway.as_deref());

        match probe_result {
            ProbeResult::OnlineUnlocked => {
                println!("Already connected and authenticated!");
                return Ok(());
            }

            ProbeResult::LockedPortal {
                gateway_base,
                login_xml_url,
            } => {
                println!("Portal detected at: {}", gateway_base);
                probe::post_login(&client, &login_xml_url, &username, &password)?;
                set_cached_gateway(&login_xml_url);
                return Ok(());
            }

            ProbeResult::InterfaceOffline => {
                println!("Wi-Fi interface offline or disconnected. Connecting to Wi-Fi...");
                if let Err(e) = wifi::connect_wifi(None) {
                    eprintln!("Wi-Fi connection command error: {}", e);
                }

                print!("Waiting for IPv4 route...");
                io::stdout().flush()?;
                match wifi::wait_for_ipv4(Duration::from_millis(6000), Duration::from_millis(200)) {
                    Some(ip) => {
                        println!(" Connected ({})", ip);
                        let post_connect_probe =
                            probe::probe_network(&client, cached_gateway.as_deref());
                        match post_connect_probe {
                            ProbeResult::OnlineUnlocked => {
                                println!("Already connected and authenticated!");
                                return Ok(());
                            }
                            ProbeResult::LockedPortal {
                                gateway_base,
                                login_xml_url,
                            } => {
                                println!("Portal detected at: {}", gateway_base);
                                probe::post_login(&client, &login_xml_url, &username, &password)?;
                                set_cached_gateway(&login_xml_url);
                                return Ok(());
                            }
                            ProbeResult::InterfaceOffline => {
                                eprintln!("Interface still offline after Wi-Fi connect attempt.");
                                return Err(
                                    "Interface still offline after Wi-Fi connect attempt.".into()
                                );
                            }
                            ProbeResult::ProbeTimeoutOrError(e) => {
                                eprintln!("Probe failed after Wi-Fi connect: {}", e);
                                return Err(e.into());
                            }
                        }
                    }
                    None => {
                        eprintln!(" Timed out waiting for IPv4 route.");
                        return Err("Timed out waiting for IPv4 route.".into());
                    }
                }
            }

            ProbeResult::ProbeTimeoutOrError(err_msg) => {
                eprintln!("Network probe error: {}", err_msg);
                return Err(err_msg.into());
            }
        }
    }

    eprintln!("Usage:");
    eprintln!("  ccp                          Authenticate using saved credentials");
    eprintln!("  ccp set                      Save username and password");
    eprintln!("  ccp start                    Start autonomous background daemon");
    eprintln!("  ccp stop                     Stop running background daemon");
    eprintln!("  ccp status                   Display daemon state and health status");
    eprintln!("  ccp daemon [--once] [--dry-run]  Run daemon loop in foreground");
    eprintln!("  ccp autostart [enable [--now]|disable] Enable/disable OS boot startup");

    Err("Invalid command line arguments.".into())
}

fn load_credentials() -> Result<(String, String), String> {
    let path = credentials_path();

    if !Path::new(&path).exists() {
        return Err("Credentials not found. Run `ccp set` first.".to_string());
    }

    let mut file = File::open(&path).map_err(|e| format!("Failed to open credentials: {e}"))?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .map_err(|e| format!("Failed to read credentials: {e}"))?;

    let mut lines = contents.lines();
    let username = lines.next().unwrap_or("").trim().to_string();
    let password = lines.next().unwrap_or("").trim().to_string();

    if username.is_empty() || password.is_empty() {
        return Err("Username or password missing in credentials file.".to_string());
    }

    Ok((username, password))
}

fn credentials_path() -> PathBuf {
    // 1. Check legacy $HOME/.config/ccp/credentials.txt (exact original main branch path)
    if let Ok(home) = env::var("HOME") {
        let legacy = PathBuf::from(home)
            .join(".config")
            .join("ccp")
            .join("credentials.txt");
        if legacy.exists() {
            return legacy;
        }
    }

    // 2. Check dirs::home_dir()/.config/ccp/credentials.txt
    if let Some(home) = dirs::home_dir() {
        let p = home.join(".config").join("ccp").join("credentials.txt");
        if p.exists() {
            return p;
        }
    }

    // 3. Check dirs::config_dir()/ccp/credentials.txt
    if let Some(config_dir) = dirs::config_dir() {
        let p = config_dir.join("ccp").join("credentials.txt");
        if p.exists() {
            return p;
        }
    }

    // 4. Check %USERPROFILE%/.config/ccp/credentials.txt
    if let Ok(profile) = env::var("USERPROFILE") {
        let p = PathBuf::from(profile)
            .join(".config")
            .join("ccp")
            .join("credentials.txt");
        if p.exists() {
            return p;
        }
    }

    // Default destination for writing new credentials:
    if let Ok(home) = env::var("HOME") {
        return PathBuf::from(home)
            .join(".config")
            .join("ccp")
            .join("credentials.txt");
    }
    if let Some(home) = dirs::home_dir() {
        return home.join(".config").join("ccp").join("credentials.txt");
    }
    if let Ok(profile) = env::var("USERPROFILE") {
        return PathBuf::from(profile)
            .join(".config")
            .join("ccp")
            .join("credentials.txt");
    }

    PathBuf::from(".config/ccp/credentials.txt")
}

fn get_config_dir() -> PathBuf {
    let creds = credentials_path();
    if let Some(parent) = creds.parent() {
        parent.to_path_buf()
    } else {
        PathBuf::from(".config/ccp")
    }
}

fn cached_gateway_path() -> PathBuf {
    get_config_dir().join("cached_gateway.txt")
}

fn get_cached_gateway() -> Option<String> {
    let path = cached_gateway_path();
    let content = fs::read_to_string(path).ok()?;
    let trimmed = content.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn set_cached_gateway(gateway: &str) {
    let path = cached_gateway_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(path, gateway);
}

fn create_credentials_file() -> std::io::Result<File> {
    let path = credentials_path();

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
    }

    #[cfg(not(unix))]
    {
        File::create(path)
    }
}
