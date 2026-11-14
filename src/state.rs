use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::Write;
use std::path::Path;

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
pub struct DaemonState {
    pub pid: u32,
    pub last_probe_epoch: u64,
    pub last_login_epoch: u64,
    pub active_gateway: Option<String>,
    pub last_local_ip: Option<String>,
    pub failure_streak: u32,
    pub halted_bad_credentials: bool,
    pub last_status_message: String,
    pub total_logins: u64,
}

impl DaemonState {
    /// Loads the daemon state from a JSON file, or returns default if not found or corrupted
    pub fn load(path: &Path) -> Self {
        if !path.exists() {
            return Self::default();
        }

        match std::fs::read_to_string(path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Atomically persists the state to disk via a temporary file and rename
    pub fn save_atomic(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let tmp_path = path.with_extension("json.tmp");
        let json_data = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        {
            let mut file = File::create(&tmp_path)?;
            file.write_all(json_data.as_bytes())?;
            file.sync_all()?;
        }

        // On Windows, rename fails if destination already exists, so remove first if needed
        if path.exists() {
            let _ = std::fs::remove_file(path);
        }

        std::fs::rename(&tmp_path, path)?;
        Ok(())
    }

    /// Resets the circuit breaker when new credentials are saved
    pub fn reset_circuit_breaker(path: &Path) {
        if !path.exists() {
            return;
        }

        let mut state = Self::load(path);
        state.halted_bad_credentials = false;
        state.failure_streak = 0;
        state.last_status_message = "Circuit breaker reset via 'ccp set'.".to_string();
        let _ = state.save_atomic(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_daemon_state_atomic_save_load_reset() {
        let tmp_dir = std::env::temp_dir().join(format!("ccp_test_state_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp_dir);
        let state_path = tmp_dir.join("state.json");

        let state = DaemonState {
            pid: 1234,
            last_probe_epoch: 100,
            last_login_epoch: 90,
            active_gateway: Some("http://172.16.68.6:8090".to_string()),
            last_local_ip: Some("172.16.68.10".to_string()),
            failure_streak: 5,
            halted_bad_credentials: true,
            last_status_message: "Halted".to_string(),
            total_logins: 3,
        };

        // Save
        assert!(state.save_atomic(&state_path).is_ok());
        assert!(state_path.exists());

        // Load
        let loaded = DaemonState::load(&state_path);
        assert_eq!(loaded, state);

        // Reset circuit breaker
        DaemonState::reset_circuit_breaker(&state_path);
        let after_reset = DaemonState::load(&state_path);
        assert!(!after_reset.halted_bad_credentials);
        assert_eq!(after_reset.failure_streak, 0);
        assert_eq!(
            after_reset.last_status_message,
            "Circuit breaker reset via 'ccp set'."
        );
        assert_eq!(after_reset.pid, 1234);

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }
}
