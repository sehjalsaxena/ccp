# CCP — Campus Captive Portal Auto-Authenticator

When moving between Hostel and College blocks, laptops roam across different Wi-Fi access points and subnets, dropping connections and requiring repetitive captive portal logins. On Windows, default proxy discovery (WPAD), IPv6 timeouts, and console subprocess calls also cause multi-second lag.

**ccp** is a cross-platform Rust CLI and background daemon that:
- **Detects captive portals in <20ms** using Firefox's `CaptiveDetect` plain-HTTP redirect discovery (`http://detectportal.firefox.com/success.txt` -> `gstatic` -> `msftconnecttest`).
- **Works across Hostel and College subnets** automatically (supports both Palo Alto `/php/uid.php` and Sophos/Cyberoam `:8090/login.xml` portals with zero hardcoded gateway IPs).
- **Runs natively on Windows without WSL** (zero console popups, IPv4-bound sockets, automatic in-range Wi-Fi connection when disconnected).
- **Automates login in the background** (`ccp start` / `ccp autostart enable`) with 2-second subnet roam detection, wake-from-sleep recovery, and a bad-password circuit breaker that prevents campus account lockouts.

---

## Installation

### Windows (Native PowerShell — No WSL Required)
Run in PowerShell:
```powershell
irm https://raw.githubusercontent.com/smresponsibilities/ccp/main/install.ps1 | iex
```

### macOS, Linux & WSL
Run in Terminal:
```bash
curl -fsSL https://raw.githubusercontent.com/smresponsibilities/ccp/main/install.sh | bash
```

### Build From Source (Cargo)
```bash
git clone https://github.com/smresponsibilities/ccp.git
cd ccp
cargo build --release
```

---

## Quick Start (Manual Mode)

### 1. Save Your Campus Credentials (One-Time Setup)
```bash
ccp set
```
Enter your campus Wi-Fi username and password. Credentials are saved locally (`~/.config/ccp/credentials.txt` with `0600` permissions on Unix, or `%USERPROFILE%\.config\ccp\credentials.txt` on Windows).

### 2. Authenticate Anytime
```bash
ccp
```
- If already online, `ccp` exits in ~20ms (`Already connected and authenticated!`).
- If locked behind a captive portal, `ccp` discovers the current building's gateway URL and logs in immediately.
- If Wi-Fi is disconnected, `ccp` connects to your in-range saved campus Wi-Fi network, waits for an IPv4 lease, and authenticates.

---

## Full Automation (Background Daemon & Auto-Start on Boot)

Instead of running `ccp` manually every time you walk between Hostel and College or wake your laptop from sleep, run the background daemon:

```bash
# Start the silent background daemon right now
ccp start

# Enable automatic startup on system login/boot
# (Windows Registry Run key / macOS LaunchAgent / Linux systemd user service)
ccp autostart enable

# Check daemon status, active gateway, local IP, login count, and circuit breaker
ccp status

# Stop the background daemon
ccp stop

# Disable automatic startup on boot
ccp autostart disable
```

### How the Background Daemon Works
1. **2-Second Roam & Wake Detection**: Checks the local IPv4 route every 2 seconds using a zero-packet local UDP socket check (<0.1ms). The moment your IP changes between Hostel and College or after sleep, it probes and logs in immediately.
2. **Periodic Keep-Alive**: Verifies connectivity every 30 seconds while online.
3. **Account Lockout Protection (Circuit Breaker)**: If the portal rejects your password (e.g., after a campus password change), the daemon halts further login attempts immediately so your campus account is never locked out. Running `ccp set` with your new password automatically resets the circuit breaker and resumes the daemon without restarting it.

---

## How to Test & Verify

### Test 1: Portal Login & Fast-Exit Benchmark (On Campus Wi-Fi)
1. Save credentials:
   ```bash
   ccp set
   ```
2. Log out of the captive portal in your browser (or reconnect to Hostel/College Wi-Fi) and run:
   ```bash
   ccp
   ```
   Verify it prints the discovered gateway (`Portal detected at: ...`) and completes login.
3. Run `ccp` a second time while already online to measure the fast-exit latency (~20-30ms):
   - **Windows PowerShell**:
     ```powershell
     (Measure-Command { ccp }).TotalMilliseconds
     ```
   - **macOS / Linux / WSL**:
     ```bash
     time ccp
     ```

### Test 2: Auto Wi-Fi Reconnect When Disconnected
1. Open your OS Wi-Fi menu and click **Disconnect** from the campus Wi-Fi network (keep Wi-Fi hardware turned on).
2. Run:
   ```bash
   ccp
   ```
3. Verify `ccp` detects `Wi-Fi interface offline or disconnected. Connecting to Wi-Fi...`, reconnects to the in-range campus SSID, obtains an IPv4 address, and logs into the portal.

### Test 3: Background Daemon Lifecycle Test (Works On or Off Campus)
```bash
ccp start
ccp status
ccp start   # Verifies single-instance lock ("ccp daemon is already running.")
ccp stop
ccp status
```

### Test 4: Automated Test Suite (37 Unit & Mock Portal Integration Tests — Works Off Campus)
Spins up local TCP mock servers simulating Firefox 302 redirects, Palo Alto `/php/uid.php` and Sophos `:8090/login.xml` authentication, URL-encoding of special characters in passwords, circuit-breaker halting on invalid passwords, and single-instance kernel locking:
```bash
cargo test --all-targets -- --nocapture
```
