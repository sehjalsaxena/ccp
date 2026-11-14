use ccp::daemon::run_daemon;
use ccp::instance::{acquire, is_daemon_running};
use ccp::state::DaemonState;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Mutex;

static TEST_MUTEX: Mutex<()> = Mutex::new(());

#[test]
fn test_integration_instance_lock_mutual_exclusion() {
    let _guard = TEST_MUTEX.lock().unwrap();
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
    let _guard = TEST_MUTEX.lock().unwrap();
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
    let _guard = TEST_MUTEX.lock().unwrap();
    let tmp_dir = std::env::temp_dir().join(format!("ccp_test_int_daemon_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&tmp_dir);

    // Run daemon in once & dry_run mode
    let res = run_daemon(&tmp_dir, "testuser", "testpassword", true, true);
    assert!(res.is_ok(), "daemon once dry_run should succeed: {:?}", res);

    let state = DaemonState::load(&tmp_dir.join("state.json"));
    assert_eq!(state.pid, std::process::id());

    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn test_integration_daemon_circuit_breaker_on_bad_password() {
    let _guard = TEST_MUTEX.lock().unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind mock server");
    let port = listener.local_addr().unwrap().port();
    let probe_url_str = format!("http://127.0.0.1:{port}/probe");

    let handle = std::thread::spawn(move || {
        // Step 1: Probe request -> 302 Found redirect to /login.xml
        if let Ok((mut stream1, _)) = listener.accept() {
            let mut buf = [0u8; 2048];
            let _ = stream1.read(&mut buf);
            let redirect_response = format!(
                "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:{port}/httpclient.html\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            let _ = stream1.write_all(redirect_response.as_bytes());
            let _ = stream1.flush();
        }

        // Step 2: Accept POST /login.xml and reply with Sophos failure XML
        if let Ok((mut stream2, _)) = listener.accept() {
            let mut buf = [0u8; 4096];
            let _ = stream2.read(&mut buf);
            let xml_body = "<requestresponse><status><![CDATA[FAIL]]></status><message><![CDATA[Make sure your password is correct]]></message></requestresponse>";
            let login_response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                xml_body.len(),
                xml_body
            );
            let _ = stream2.write_all(login_response.as_bytes());
            let _ = stream2.flush();
        }
    });

    let tmp_dir =
        std::env::temp_dir().join(format!("ccp_test_circuit_breaker_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&tmp_dir);

    unsafe {
        std::env::set_var("CCP_PROBE_URL", &probe_url_str);
    }
    let res = run_daemon(&tmp_dir, "testuser", "badpass", true, false);
    unsafe {
        std::env::remove_var("CCP_PROBE_URL");
    }

    assert!(
        res.is_ok(),
        "run_daemon should succeed cleanly on once=true: {:?}",
        res
    );
    handle.join().unwrap();

    let state_file = tmp_dir.join("state.json");
    let state = DaemonState::load(&state_file);
    assert!(
        state.halted_bad_credentials,
        "Circuit breaker should be halted on bad credentials"
    );
    assert_eq!(state.failure_streak, 1);

    let _ = std::fs::remove_dir_all(&tmp_dir);
}
