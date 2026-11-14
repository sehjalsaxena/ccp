use ccp::daemon::run_daemon;
use ccp::instance::{acquire, is_daemon_running};
use ccp::state::DaemonState;

#[test]
fn test_integration_instance_lock_mutual_exclusion() {
    let tmp_dir = std::env::temp_dir().join(format!("ccp_test_int_lock_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&tmp_dir);
    let lock_file = tmp_dir.join("daemon.lock");

    let _ = std::fs::remove_file(&lock_file);
    assert!(!is_daemon_running(&lock_file));

    let lock1 = acquire(&lock_file).expect("Failed to acquire lock1");
    assert!(is_daemon_running(&lock_file));

    let lock2 = acquire(&lock_file);
    assert!(lock2.is_err(), "Second acquire should fail");

    drop(lock1);
    assert!(!is_daemon_running(&lock_file));

    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn test_integration_state_atomic_and_circuit_breaker() {
    let tmp_dir = std::env::temp_dir().join(format!("ccp_test_int_state_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&tmp_dir);
    let state_file = tmp_dir.join("state.json");

    let state = DaemonState {
        pid: 9999,
        last_probe_epoch: 500,
        last_login_epoch: 400,
        active_gateway: Some("http://10.1.1.9:8090/login.xml".to_string()),
        last_local_ip: Some("10.1.1.50".to_string()),
        failure_streak: 4,
        halted_bad_credentials: true,
        last_status_message: "Halted (bad credentials)".to_string(),
        total_logins: 12,
    };

    assert!(state.save_atomic(&state_file).is_ok());

    let loaded = DaemonState::load(&state_file);
    assert_eq!(loaded.pid, 9999);
    assert!(loaded.halted_bad_credentials);
    assert_eq!(loaded.failure_streak, 4);

    DaemonState::reset_circuit_breaker(&state_file);
    let after_reset = DaemonState::load(&state_file);
    assert!(!after_reset.halted_bad_credentials);
    assert_eq!(after_reset.failure_streak, 0);

    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn test_integration_daemon_once_dry_run() {
    let tmp_dir = std::env::temp_dir().join(format!("ccp_test_int_daemon_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&tmp_dir);

    // Run daemon in once & dry_run mode
    let res = run_daemon(&tmp_dir, "testuser", "testpassword", true, true);
    assert!(res.is_ok(), "daemon once dry_run should succeed: {:?}", res);

    let state = DaemonState::load(&tmp_dir.join("state.json"));
    assert_eq!(state.pid, std::process::id());

    let _ = std::fs::remove_dir_all(&tmp_dir);
}
