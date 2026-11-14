use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Kernel-enforced single-instance process lock.
pub struct SingleInstanceLock {
    _file: File,
    path: PathBuf,
}

impl SingleInstanceLock {
    pub fn acquire(lock_path: &Path) -> Result<Self, String> {
        acquire(lock_path)
    }
}

impl Drop for SingleInstanceLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(target_os = "windows")]
pub fn acquire(lock_path: &Path) -> Result<SingleInstanceLock, String> {
    use std::os::windows::fs::OpenOptionsExt;

    if let Some(parent) = lock_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    // share_mode(1) grants FILE_SHARE_READ but denies FILE_SHARE_WRITE.
    // Any second writer fails with ERROR_SHARING_VIOLATION (32), while readers can read the PID!
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .share_mode(1)
        .open(lock_path)
        .map_err(|e| {
            if e.raw_os_error() == Some(32) || e.kind() == std::io::ErrorKind::PermissionDenied {
                "Daemon is already running (exclusive file lock held).".to_string()
            } else {
                format!("Failed to acquire instance lock: {e}")
            }
        })?;

    let pid = std::process::id();
    let _ = writeln!(file, "{pid}");
    let _ = file.flush();

    Ok(SingleInstanceLock {
        _file: file,
        path: lock_path.to_path_buf(),
    })
}

#[cfg(not(target_os = "windows"))]
pub fn acquire(lock_path: &Path) -> Result<SingleInstanceLock, String> {
    use std::process::Command;

    if let Some(parent) = lock_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    if lock_path.exists() {
        if let Ok(content) = std::fs::read_to_string(lock_path) {
            let pid_str = content.trim();
            if !pid_str.is_empty() {
                let is_alive = Command::new("kill")
                    .args(["-0", pid_str])
                    .output()
                    .map(|out| out.status.success())
                    .unwrap_or(false);

                if is_alive {
                    return Err(format!("Daemon is already running (PID {pid_str})."));
                }
            }
        }
    }

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .open(lock_path)
        .map_err(|e| format!("Failed to create instance lock: {e}"))?;

    let pid = std::process::id();
    let payload = format!("{pid}\n");
    let _ = file.write_all(payload.as_bytes());
    let _ = file.set_len(payload.len() as u64);
    let _ = file.flush();

    Ok(SingleInstanceLock {
        _file: file,
        path: lock_path.to_path_buf(),
    })
}

#[cfg(target_os = "windows")]
pub fn is_daemon_running(lock_path: &Path) -> bool {
    use std::os::windows::fs::OpenOptionsExt;

    if !lock_path.exists() {
        return false;
    }

    // Attempt to open with write access and share_mode(3) (FILE_SHARE_READ | FILE_SHARE_WRITE).
    // If the daemon holds share_mode(1), write access is denied with ERROR_SHARING_VIOLATION (32).
    match std::fs::OpenOptions::new()
        .write(true)
        .share_mode(3)
        .open(lock_path)
    {
        Ok(_) => {
            // Lock was available for writing, so daemon is not running
            false
        }
        Err(e) => {
            // Error 32: ERROR_SHARING_VIOLATION -> daemon is holding write lock
            e.raw_os_error() == Some(32) || e.kind() == std::io::ErrorKind::PermissionDenied
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn is_daemon_running(lock_path: &Path) -> bool {
    use std::process::Command;

    if !lock_path.exists() {
        return false;
    }

    if let Ok(content) = std::fs::read_to_string(lock_path) {
        let pid_str = content.trim();
        if !pid_str.is_empty() {
            return Command::new("kill")
                .args(["-0", pid_str])
                .output()
                .map(|out| out.status.success())
                .unwrap_or(false);
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_instance_lock_lifecycle() {
        let tmp_dir = std::env::temp_dir().join(format!("ccp_test_lock_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp_dir);
        let lock_file = tmp_dir.join("test_daemon.lock");

        // Clean slate
        let _ = std::fs::remove_file(&lock_file);
        assert!(!is_daemon_running(&lock_file));

        // Acquire lock 1
        let lock1 = acquire(&lock_file);
        assert!(lock1.is_ok(), "First acquire should succeed");
        assert!(is_daemon_running(&lock_file), "Lock should now be active");

        // Acquire lock 2 while lock 1 is alive -> MUST fail
        let lock2 = acquire(&lock_file);
        assert!(
            lock2.is_err(),
            "Second acquire should fail while lock1 is held"
        );

        // Drop lock 1
        drop(lock1);

        // Now lock should be released
        assert!(!is_daemon_running(&lock_file));

        // Re-acquire should succeed again
        let lock3 = acquire(&lock_file);
        assert!(lock3.is_ok());
        drop(lock3);

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }
}
