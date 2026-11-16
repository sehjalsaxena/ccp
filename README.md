# CCP — Campus Captive Portal Auto-Authenticator

When moving between Hostel and College blocks, laptops roam across different Wi-Fi access points and subnets, dropping connections and requiring repetitive captive portal logins. On Windows, default proxy discovery (WPAD), IPv6 timeouts, and console subprocess calls also cause multi-second lag.

**ccp** is a cross-platform CLI and background daemon that:
- **Detects captive portals in <20ms** using Firefox's `CaptiveDetect` plain-HTTP redirect discovery (`http://detectportal.firefox.com/success.txt` -> `gstatic` -> `msftconnecttest`).
- **Works across Hostel and College subnets** automatically (supports both Palo Alto `/php/uid.php` and Sophos/Cyberoam `:8090/login.xml` portals with zero hardcoded gateway IPs).
- **Runs natively on Windows without WSL** (zero console popups, IPv4-bound sockets, automatic in-range Wi-Fi connection when disconnected).
- **Automates login in the background** (`ccp start` / `ccp autostart enable`) with 2-second subnet roam detection, wake-from-sleep recovery, and a bad-password circuit breaker that prevents campus account lockouts.

---

## 30-Second Quick Setup (Windows — No Dependencies)

Just open PowerShell and run:
```powershell
irm https://raw.githubusercontent.com/smresponsibilities/ccp/main/install.ps1 | iex
ccp set
ccp start
ccp autostart enable
```
> **No Rust, no Cargo, no Git, and no WSL required.**

---

## Installation

### 1. Windows (One-Line PowerShell Install)
Open PowerShell and run:
```powershell
irm https://raw.githubusercontent.com/smresponsibilities/ccp/main/install.ps1 | iex
```
*`ccp` is downloaded, installed, and added to your PATH immediately—usable in the same terminal.*

### 2. Direct Windows `.exe` Download
Download [`ccp-windows-amd64.zip`](https://github.com/smresponsibilities/ccp/releases/latest/download/ccp-windows-amd64.zip), extract `ccp.exe`, and double-click or run from command prompt.

### 3. macOS, Linux & WSL (One-Line Install)
Open Terminal and run:
```bash
curl -fsSL https://raw.githubusercontent.com/smresponsibilities/ccp/main/install.sh | bash
```

### 4. Build From Source (Cargo)
For developers or users compiling from source:
```bash
git clone https://github.com/smresponsibilities/ccp.git
cd ccp
cargo build --release
```

---

## Usage Methods

Choose the mode that fits your routine best:

---

### Method 1: Simple Manual Mode (Zero Background Processes)
If you prefer not having any background process running at all:
1. **Save your credentials once:**
   ```bash
   ccp set
   ```
2. **Authenticate whenever needed:**
   ```bash
   ccp
   ```
   - If already online: exits in ~20ms (`Already connected and authenticated!`).
   - If behind a captive portal: detects the gateway and logs in immediately.
   - If disconnected from Wi-Fi: automatically finds and connects to the campus SSID, gets an IPv4 address, and logs in.

---

### Method 2: Background Daemon (Current Session Only — Without Startup)
If you want automatic roaming between Hostel and College during your current session, but don't want it starting automatically on system boot:
```bash
# Start background roaming daemon right now
ccp start

# Check real-time daemon state, local IP, gateway, and login count
ccp status

# Stop background daemon whenever you want
ccp stop
```

---

### Method 3: Full Auto-Start on System Boot (Permanent "Set & Forget")
If you never want to think about captive portals again:
```bash
# Option A: Enable boot startup AND start daemon immediately right now
ccp autostart enable --now

# Option B: Enable boot startup only (starts on next restart/login)
ccp autostart enable

# Check status anytime
ccp status

# Disable boot startup anytime
ccp autostart disable
```

---

### How the Background Daemon Works
1. **2-Second Roam & Wake Detection**: Checks the local IPv4 route every 2 seconds using a zero-packet local UDP socket check (<0.1ms). The moment your IP changes between Hostel and College or after waking from sleep, it probes and logs in immediately.
2. **Periodic Keep-Alive**: Verifies connectivity every 30 seconds while online.
3. **Account Lockout Protection (Circuit Breaker)**: If the portal rejects your password (e.g., after a campus password change), the daemon halts further login attempts immediately so your campus account is never locked out. Running `ccp set` with your new password automatically resets the circuit breaker and resumes the daemon without restarting it.

---

## Script & CLI Parameter Reference

### 1. Installation Scripts Parameters

| Script | Parameter / Env Variable | Default | Description |
| :--- | :--- | :--- | :--- |
| **`install.ps1`** | `-Version` (`$env:CCP_VERSION`) | Latest release | Specifies release tag to install (e.g. `v0.2.0`). |
| | `-Repo` (`$env:CCP_REPO`) | `smresponsibilities/ccp` | Target GitHub repository (`owner/repo`). |
| | `-InstallDir` (`$env:INSTALL_DIR`) | `$env:USERPROFILE\.ccp\bin` | Directory where `ccp.exe` is placed and added to PATH. |
| **`install.sh`** | `CCP_VERSION` | Latest release | Specifies release tag to install (e.g. `v0.2.0`). |
| | `CCP_REPO` | `smresponsibilities/ccp` | Target GitHub repository (`owner/repo`). |
| | `INSTALL_DIR` | `$HOME/.local/bin` | Directory where `ccp` binary is placed. |

### 2. CLI Commands & Options

| Command | Arguments / Flags | Description | Example |
| :--- | :--- | :--- | :--- |
| **`ccp`** | *(none)* | Manual authentication mode. Discovers captive portal and logs in immediately, or fast-exits in ~20ms if already connected. | `ccp` |
| **`ccp set`** | *(none)* | Saves campus Wi-Fi username and password locally (`0600` permissions on Unix). Resets circuit breaker. | `ccp set` |
| **`ccp start`** | *(none)* | Starts the autonomous background daemon detached in the background with no console window. | `ccp start` |
| **`ccp stop`** | *(none)* | Authoritatively stops the running background daemon and releases kernel locks. | `ccp stop` |
| **`ccp status`** | *(none)* | Displays real-time operational status, PID, detected local IPv4, active gateway, login count, and circuit breaker. | `ccp status` |
| **`ccp autostart`** | `enable` | Registers auto-start on boot (Windows Registry / macOS LaunchAgent / Linux systemd). Starts on next login/boot. | `ccp autostart enable` |
| | `enable --now` (or `-n`) | Registers auto-start on boot **AND** immediately launches the background daemon right now without waiting for reboot. | `ccp autostart enable --now` |
| | `disable` | Disables and cleans up OS boot auto-start registration. | `ccp autostart disable` |
| **`ccp daemon`** | *(none)* | Runs daemon roaming loop in foreground (polls route every 2s, keep-alive every 30s). | `ccp daemon` |
| | `--once` | Runs one single check iteration and exits immediately. | `ccp daemon --once` |
| | `--dry-run` | Probes network and discovers portal without submitting login credentials. | `ccp daemon --once --dry-run` |

---

## How to Test & Verify

### Test 1: Real-Life Auto-Start Test (Windows — No Code Required)
To verify that Windows itself launches `ccp` automatically on system login:
1. Enable autostart and stop the daemon so you know it's off:
   ```powershell
   ccp autostart enable
   ccp stop
   ccp status    # Displays: State: Stopped
   ```
2. **Sign Out of Windows**: Press `Win + X` → **Shut down or sign out** → **Sign out** (or press `Ctrl + Alt + Del` → **Sign out**).
3. Sign back into Windows to reach your desktop.
4. Open PowerShell and check status:
   ```powershell
   ccp status
   ```
   **Expected Output:** `State: Running` with an active PID and detected Local IP. This proves Windows launched `ccp` in the background on startup!
5. **Visual check via Task Manager**: Press `Ctrl + Shift + Esc` → click the **Startup apps** tab → look for **`CCP_AutoAuth`** with status **`Enabled`**.

### Test 2: Background Daemon Lifecycle Test (Works On or Off Campus)
```bash
ccp start
ccp status
ccp start   # Verifies single-instance lock ("ccp daemon is already running.")
ccp stop
ccp status
```

### Test 3: Foreground Dry-Run Verification (Safe Simulation)
To test portal detection logic in the foreground without sending login credentials or starting background services:
```bash
ccp daemon --once --dry-run
```

### Test 4: Portal Login & Fast-Exit Benchmark (On Campus Wi-Fi)
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

### Test 5: Auto Wi-Fi Reconnect When Disconnected
1. Open your OS Wi-Fi menu and click **Disconnect** from the campus Wi-Fi network (keep Wi-Fi hardware turned on).
2. Run:
   ```bash
   ccp
   ```
3. Verify `ccp` detects `Wi-Fi interface offline or disconnected. Connecting to Wi-Fi...`, reconnects to the in-range campus SSID, obtains an IPv4 address, and logs into the portal.

### Test 6: Automated Test Suite (37 Unit & Mock Portal Integration Tests — Works Off Campus)
For developers verifying the codebase: spins up local TCP mock servers simulating Firefox 302 redirects, Palo Alto `/php/uid.php` and Sophos `:8090/login.xml` authentication, URL-encoding of special characters in passwords, circuit-breaker halting on invalid passwords, and single-instance kernel locking:
```bash
cargo test --all-targets -- --nocapture
```
