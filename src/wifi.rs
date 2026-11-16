use std::net::Ipv4Addr;
use std::time::{Duration, Instant};

/// Polls for a valid local IPv4 route every `poll_interval` until `timeout` expires
pub fn wait_for_ipv4(timeout: Duration, poll_interval: Duration) -> Option<Ipv4Addr> {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if let Some(ip) = crate::probe::current_local_ipv4() {
            return Some(ip);
        }
        std::thread::sleep(poll_interval);
    }
    None
}

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[cfg(target_os = "windows")]
pub fn connect_wifi(ssid: Option<&str>) -> std::io::Result<()> {
    use std::process::Command;

    let target_profile = if let Some(s) = ssid {
        s.to_string()
    } else {
        // Find existing WLAN profiles
        let profiles_output = Command::new("netsh")
            .args(["wlan", "show", "profiles"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()?;
        let profiles_stdout = String::from_utf8_lossy(&profiles_output.stdout);

        // Find visible networks currently in-range
        let networks_output = Command::new("netsh")
            .args(["wlan", "show", "networks"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .ok();
        let networks_stdout =
            networks_output.map(|o| String::from_utf8_lossy(&o.stdout).to_string());

        find_preferred_wlan_profile_in_range(&profiles_stdout, networks_stdout.as_deref())
            .unwrap_or_default()
    };

    if !target_profile.is_empty() {
        let _ = Command::new("netsh")
            .args(["wlan", "connect", &format!("name={}", target_profile)])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
    }

    Ok(())
}

#[cfg(target_os = "windows")]
pub fn find_preferred_wlan_profile(profiles_output: &str) -> Option<String> {
    find_preferred_wlan_profile_in_range(profiles_output, None)
}

#[cfg(target_os = "windows")]
pub fn find_preferred_wlan_profile_in_range(
    profiles_output: &str,
    networks_output: Option<&str>,
) -> Option<String> {
    let mut profiles = Vec::new();
    let mut in_profiles_section = false;

    for line in profiles_output.lines() {
        let trimmed = line.trim();
        if trimmed.contains("----") {
            in_profiles_section = true;
            continue;
        }

        if let Some(idx) = line.find(':') {
            let left = line[..idx].trim().to_lowercase();
            let right = line[idx + 1..].trim();

            if in_profiles_section {
                if !right.is_empty() && right != "<None>" && right != "<none>" {
                    profiles.push(right.to_string());
                }
            } else if (left.contains("profile") || left.contains("profil"))
                && !right.is_empty()
                && right != "<None>"
                && right != "<none>"
            {
                profiles.push(right.to_string());
            }
        }
    }

    if profiles.is_empty() {
        return None;
    }

    // If networks_output is provided, filter candidate profiles to those currently in range
    let in_range_profiles: Vec<String> = if let Some(net_out) = networks_output {
        let mut visible_ssids = Vec::new();
        for line in net_out.lines() {
            if let Some(idx) = line.find(':') {
                let left = line[..idx].trim().to_lowercase();
                let right = line[idx + 1..].trim();
                if left.starts_with("ssid") && !right.is_empty() {
                    visible_ssids.push(right.to_lowercase());
                }
            }
        }

        let filtered: Vec<String> = profiles
            .iter()
            .filter(|p| visible_ssids.contains(&p.to_lowercase()))
            .cloned()
            .collect();

        if !filtered.is_empty() {
            filtered
        } else {
            profiles
        }
    } else {
        profiles
    };

    // Prioritize campus / hostel / college Wi-Fi names
    let keywords = ["hostel", "college", "campus", "wifi", "eduroam", "student"];
    for keyword in &keywords {
        if let Some(p) = in_range_profiles
            .iter()
            .find(|p| p.to_lowercase().contains(keyword))
        {
            return Some(p.clone());
        }
    }

    // Default to the first in-range profile if no keyword matches
    in_range_profiles.into_iter().next()
}

#[cfg(target_os = "linux")]
pub fn connect_wifi(ssid: Option<&str>) -> std::io::Result<()> {
    use std::process::Command;
    if let Some(s) = ssid {
        let _ = Command::new("nmcli")
            .args(["dev", "wifi", "connect", s])
            .output();
    } else {
        let _ = Command::new("nmcli").args(["radio", "wifi", "on"]).output();
        let _ = Command::new("nmcli").args(["networking", "on"]).output();
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn get_macos_wifi_device() -> String {
    use std::process::Command;
    if let Ok(output) = Command::new("networksetup")
        .arg("-listallhardwareports")
        .output()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut found_wifi = false;
        for line in stdout.lines() {
            let trimmed = line.trim();
            if trimmed.contains("Wi-Fi") || trimmed.contains("AirPort") {
                found_wifi = true;
            } else if found_wifi
                && trimmed.starts_with("Device:")
                && let Some(dev) = trimmed.split(':').nth(1)
            {
                let dev = dev.trim();
                if !dev.is_empty() {
                    return dev.to_string();
                }
            }
        }
    }
    "en0".to_string()
}

#[cfg(target_os = "macos")]
pub fn connect_wifi(ssid: Option<&str>) -> std::io::Result<()> {
    use std::process::Command;
    let dev = get_macos_wifi_device();
    if let Some(s) = ssid {
        let _ = Command::new("networksetup")
            .args(["-setairportnetwork", &dev, s])
            .output();
    } else {
        let _ = Command::new("networksetup")
            .args(["-setairportpower", &dev, "on"])
            .output();
    }
    Ok(())
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
pub fn connect_wifi(_ssid: Option<&str>) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "windows")]
    use super::*;

    #[test]
    #[cfg(target_os = "windows")]
    fn test_find_preferred_wlan_profile() {
        let sample = "
Profiles on interface Wi-Fi:

Group policy profiles (read only)
---------------------------------
    <None>

User profiles
-------------
    All User Profile     : Guest_WiFi
    All User Profile     : Campus_Hostel_Net
    All User Profile     : Home_Network
";
        let found = find_preferred_wlan_profile(sample);
        assert_eq!(found, Some("Campus_Hostel_Net".to_string()));
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn test_find_preferred_wlan_profile_in_range_filtering() {
        let profiles = "
User profiles
-------------
    All User Profile     : Campus_Hostel_Net
    All User Profile     : College_Classroom_WiFi
    All User Profile     : Home_Network
";
        let networks = "
Interface name : Wi-Fi 
There are 2 networks currently visible. 

SSID 1 : College_Classroom_WiFi
    Network type            : Infrastructure
SSID 2 : Random_Cafe
    Network type            : Infrastructure
";
        // When at college, Campus_Hostel_Net is out of range, so College_Classroom_WiFi must be chosen!
        let found = find_preferred_wlan_profile_in_range(profiles, Some(networks));
        assert_eq!(found, Some("College_Classroom_WiFi".to_string()));
    }
}
