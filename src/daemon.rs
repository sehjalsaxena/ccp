use crate::instance::{self, SingleInstanceLock};
use crate::probe::{self, ProbeResult};
use crate::state::DaemonState;
use crate::wifi;
use std::path::Path;
use std::time::Duration;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
#[cfg(target_os = "windows")]
const DETACHED_PROCESS: u32 = 0x0000_0008;

/// Runs the autonomous background daemon roaming & re-auth loop.
pub fn run_daemon(
    config_dir: &Path,
    username: &str,
    password: &str,
    once: bool,
    dry_run: bool,
) -> Result<(), String> {
    let lock_path = config_dir.join("daemon.lock");
    let state_path = config_dir.join("state.json");

    // Skip kernel instance lock only when running a one-shot dry-run check
    let _lock = if once && dry_run {
        None
    } else {
        Some(SingleInstanceLock::acquire(&lock_path)?)
    };

    let mut state = DaemonState::load(&state_path);
    state.pid = std::process::id();
    let _ = state.save_atomic(&state_path);

    let client = probe::build_fast_client(false).map_err(|e| e.to_string())?;
    let mut disconnected_ticks: u32 = 0;

    loop {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let mut force_probe = once;
        let local_ip = probe::current_local_ipv4();

        // 1. Fast Roaming Check (<0.1ms, zero packets)
        match local_ip {
            None => {
                state.last_local_ip = None;
                disconnected_ticks += 1;
                // Every 5 ticks (~10s), attempt Wi-Fi reconnection if disconnected
                if disconnected_ticks >= 5 {
                    disconnected_ticks = 0;
                    if !dry_run {
                        let _ = wifi::connect_wifi(None);
                        if let Some(ip) =
                            wifi::wait_for_ipv4(Duration::from_secs(5), Duration::from_millis(200))
                        {
                            state.last_local_ip = Some(ip.to_string());
                            force_probe = true;
                        }
                    }
                }
            }
            Some(ip) => {
                disconnected_ticks = 0;
                let ip_str = ip.to_string();
                if state.last_local_ip.as_deref() != Some(&ip_str) {
                    // Subnet Roam / Wake-from-Sleep Detected!
                    state.last_local_ip = Some(ip_str);
                    force_probe = true;
                }
            }
        }

        // 2. Portal Probe (when forced, or every 30s)
        if force_probe || now.saturating_sub(state.last_probe_epoch) >= 30 {
            state.last_probe_epoch = now;

            let probe_res = probe::probe_network(&client, state.active_gateway.as_deref());
            match probe_res {
                ProbeResult::OnlineUnlocked => {
                    state.failure_streak = 0;
                    state.last_status_message = "Online and unlocked".to_string();
                    let _ = state.save_atomic(&state_path);
                }

                ProbeResult::LockedPortal {
                    gateway_base,
                    login_xml_url,
                } => {
                    if dry_run {
                        state.last_status_message =
                            format!("Dry-run: portal locked at {gateway_base}");
                        let _ = state.save_atomic(&state_path);
                        if once {
                            return Ok(());
                        }
                    } else if state.halted_bad_credentials {
                        // Circuit breaker engaged: prevent campus account lockouts
                        state.last_status_message =
                            "Halted due to bad credentials. Run 'ccp set'.".to_string();
                        let _ = state.save_atomic(&state_path);
                    } else {
                        match probe::post_login(&client, &login_xml_url, username, password) {
                            Ok(_) => {
                                state.failure_streak = 0;
                                state.total_logins += 1;
                                state.last_login_epoch = now;
                                state.active_gateway = Some(login_xml_url);
                                state.last_status_message =
                                    format!("Authenticated at {gateway_base}");
                                let _ = state.save_atomic(&state_path);
                            }
                            Err(e) => {
                                let err_str = e.to_string();
                                let err_lower = err_str.to_lowercase();
                                let is_credential_err = [
                                    "invalid username",
                                    "invalid password",
                                    "incorrect password",
                                    "wrong password",
                                    "invalid credentials",
                                    "make sure your password is correct",
                                ]
                                .iter()
                                .any(|k| err_lower.contains(k));

                                if is_credential_err {
                                    state.halted_bad_credentials = true;
                                    state.failure_streak += 1;
                                    state.last_status_message =
                                        format!("Halted (bad credentials): {err_str}");
                                    let _ = state.save_atomic(&state_path);
                                } else {
                                    state.failure_streak += 1;
                                    state.last_status_message = format!(
                                        "Login failed (attempt {}): {err_str}",
                                        state.failure_streak
                                    );
                                    let _ = state.save_atomic(&state_path);

                                    // Exponential backoff up to 300s
                                    let backoff = 300u64.min(
                                        5 * (1u64
                                            << (state.failure_streak.saturating_sub(1).min(6))),
                                    );
                                    std::thread::sleep(Duration::from_secs(backoff));
                                }
                            }
                        }
                    }
                }

                ProbeResult::InterfaceOffline => {
                    state.last_status_message = "Wi-Fi interface offline".to_string();
                    let _ = state.save_atomic(&state_path);
                }

                ProbeResult::ProbeTimeoutOrError(e) => {
                    state.last_status_message = format!("Probe error: {e}");
                    let _ = state.save_atomic(&state_path);
                }
            }
        }

        if once {
            return Ok(());
        }

        std::thread::sleep(Duration::from_secs(2));
    }
}

/// Spawns ccp in the background completely detached with no window.
pub fn start_background_daemon(config_dir: &Path) -> Result<(), String> {
    let lock_path = config_dir.join("daemon.lock");
    if instance::is_daemon_running(&lock_path) {
        println!("ccp daemon is already running.");
        return Ok(());
    }

    let exe = std::env::current_exe().map_err(|e| format!("Failed to get exe path: {e}"))?;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("daemon")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());

    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS);

    cmd.spawn()
        .map_err(|e| format!("Failed to spawn background daemon: {e}"))?;

    println!("Background daemon started successfully.");
    Ok(())
}

/// Stops the running background daemon by terminating its process.
pub fn stop_daemon(config_dir: &Path) -> Result<(), String> {
    let lock_path = config_dir.join("daemon.lock");
    let state_path = config_dir.join("state.json");

    if !instance::is_daemon_running(&lock_path) {
        println!("No active ccp daemon is running.");
        return Ok(());
    }

    let state = DaemonState::load(&state_path);
    let mut pid = state.pid;

    if pid == 0
        && lock_path.exists()
        && let Ok(c) = std::fs::read_to_string(&lock_path)
    {
        pid = c.trim().parse::<u32>().unwrap_or(0);
    }

    if pid > 0 {
        #[cfg(target_os = "windows")]
        {
            let mut cmd = std::process::Command::new("taskkill");
            cmd.args(["/PID", &pid.to_string(), "/F"])
                .creation_flags(CREATE_NO_WINDOW);
            let _ = cmd.output();
        }

        #[cfg(not(target_os = "windows"))]
        {
            let _ = std::process::Command::new("kill")
                .arg(pid.to_string())
                .output();
        }

        println!("Daemon (PID {pid}) stopped.");
    } else {
        println!("Unable to determine PID of running daemon.");
    }

    let _ = std::fs::remove_file(&lock_path);
    Ok(())
}

/// Prints the detailed operational status of the daemon.
pub fn print_status(config_dir: &Path) {
    let lock_path = config_dir.join("daemon.lock");
    let state_path = config_dir.join("state.json");

    let is_running = instance::is_daemon_running(&lock_path);
    let state = DaemonState::load(&state_path);

    let status_str = if is_running { "Running" } else { "Stopped" };
    let breaker_str = if state.halted_bad_credentials {
        "Halted (Bad Credentials - run 'ccp set')"
    } else {
        "Normal (Active)"
    };

    println!("CCP Daemon Status");
    println!("-----------------");
    println!("State:            {status_str}");
    if is_running && state.pid > 0 {
        println!("PID:              {}", state.pid);
    }
    println!(
        "Local IP:         {}",
        state.last_local_ip.as_deref().unwrap_or("None (Offline)")
    );
    println!(
        "Active Gateway:   {}",
        state
            .active_gateway
            .as_deref()
            .unwrap_or("None (Undiscovered)")
    );
    println!("Total Logins:     {}", state.total_logins);
    println!("Failure Streak:   {}", state.failure_streak);
    println!("Circuit Breaker:  {breaker_str}");
    println!(
        "Last Status:      {}",
        if state.last_status_message.is_empty() {
            "No activity recorded"
        } else {
            &state.last_status_message
        }
    );
}

/// Configures operating system autostart on system boot/login.
pub fn set_autostart(enable: bool) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("Failed to get exe path: {e}"))?;
    let exe_str = exe.to_string_lossy();

    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        if enable {
            let value = format!("\"{exe_str}\" start");
            let output = Command::new("reg")
                .args([
                    "add",
                    r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                    "/v",
                    "CCP_AutoAuth",
                    "/t",
                    "REG_SZ",
                    "/d",
                    &value,
                    "/f",
                ])
                .creation_flags(CREATE_NO_WINDOW)
                .output()
                .map_err(|e| format!("Failed to execute reg.exe: {e}"))?;

            if output.status.success() {
                println!("[OK] Auto-start enabled (Windows Startup registry key set).");
            } else {
                return Err(format!(
                    "Failed to set registry key: {}",
                    String::from_utf8_lossy(&output.stderr)
                ));
            }
        } else {
            let output = Command::new("reg")
                .args([
                    "delete",
                    r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                    "/v",
                    "CCP_AutoAuth",
                    "/f",
                ])
                .creation_flags(CREATE_NO_WINDOW)
                .output()
                .map_err(|e| format!("Failed to execute reg.exe: {e}"))?;

            if output.status.success() {
                println!("[OK] Auto-start disabled (Windows Startup registry key removed).");
            } else {
                return Err(format!(
                    "Failed to delete registry key: {}",
                    String::from_utf8_lossy(&output.stderr)
                ));
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME").map_err(|_| "HOME not set".to_string())?;
        let agents_dir = Path::new(&home).join("Library/LaunchAgents");
        let plist_path = agents_dir.join("com.garvittsingla.ccp.plist");

        if enable {
            std::fs::create_dir_all(&agents_dir)
                .map_err(|e| format!("Failed to create LaunchAgents dir: {e}"))?;
            let plist_content = format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.garvittsingla.ccp</string>
    <key>ProgramArguments</key>
    <array>
        <string>{exe_str}</string>
        <string>start</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <false/>
</dict>
</plist>"#
            );
            std::fs::write(&plist_path, plist_content)
                .map_err(|e| format!("Failed to write plist: {e}"))?;
            println!("[OK] Auto-start enabled (macOS LaunchAgent created).");
        } else if plist_path.exists() {
            let _ = std::fs::remove_file(&plist_path);
            println!("[OK] Auto-start disabled (macOS LaunchAgent removed).");
        }
    }

    #[cfg(target_os = "linux")]
    {
        let home = std::env::var("HOME").map_err(|_| "HOME not set".to_string())?;
        let systemd_dir = Path::new(&home).join(".config/systemd/user");
        let service_path = systemd_dir.join("ccp.service");

        if enable {
            std::fs::create_dir_all(&systemd_dir)
                .map_err(|e| format!("Failed to create systemd user dir: {e}"))?;
            let service_content = format!(
                "[Unit]\nDescription=CCP Captive Portal Daemon\nAfter=network.target\n\n[Service]\nExecStart={exe_str} daemon\nRestart=always\nRestartSec=5\n\n[Install]\nWantedBy=default.target\n"
            );
            std::fs::write(&service_path, service_content)
                .map_err(|e| format!("Failed to write service file: {e}"))?;
            let _ = std::process::Command::new("systemctl")
                .args(["--user", "enable", "--now", "ccp.service"])
                .output();
            println!("[OK] Auto-start enabled (Linux user systemd service enabled).");
        } else {
            let _ = std::process::Command::new("systemctl")
                .args(["--user", "disable", "--now", "ccp.service"])
                .output();
            let _ = std::fs::remove_file(&service_path);
            println!("[OK] Auto-start disabled (Linux user systemd service removed).");
        }
    }

    Ok(())
}
