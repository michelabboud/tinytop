use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[test]
fn serve_exposes_dashboard_and_history_api() {
    let port = reserve_port();
    let public_dir = temp_public_dir();
    let mut child = Command::new(env!("CARGO_BIN_EXE_tinytop-agent"))
        .args([
            "serve",
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--sqlite",
            "sqlite::memory:",
            "--poll-ms",
            "100000",
            "--public-dir",
            public_dir
                .to_str()
                .expect("public path should be valid UTF-8"),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("tinytop-agent serve should start");

    let result = wait_for_server(port)
        .and_then(|_| http_get(port, "/health"))
        .map(|response| assert!(response.contains("\r\n\r\nok")))
        .and_then(|_| http_get(port, "/"))
        .map(|response| assert!(response.contains("TinyTop test dashboard")))
        .and_then(|_| http_get(port, "/api/history?limit=1"))
        .map(|response| assert!(response.contains("\"samples\"")));

    stop_child(&mut child);
    fs::remove_dir_all(public_dir).ok();

    result.expect("server should expose dashboard and history API");
}

fn reserve_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("should reserve a local port");
    listener
        .local_addr()
        .expect("reserved listener should have an address")
        .port()
}

fn temp_public_dir() -> std::path::PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after epoch")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("tinytop-agent-public-{stamp}"));
    fs::create_dir_all(dir.join("vendor")).expect("public vendor directory should be created");
    fs::write(dir.join("index.html"), "TinyTop test dashboard").expect("index should be written");
    fs::write(dir.join("styles.css"), "body{}").expect("styles should be written");
    fs::write(dir.join("app.js"), "console.log('test')").expect("app should be written");
    fs::write(dir.join("vendor/echarts.min.js"), "window.echarts = {};")
        .expect("vendor script should be written");
    dir
}

fn wait_for_server(port: u16) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if let Ok(response) = http_get(port, "/health") {
            if response.contains("\r\n\r\nok") {
                return Ok(());
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err("server did not become healthy".to_string())
}

fn http_get(port: u16, path: &str) -> Result<String, String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).map_err(|error| error.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|error| error.to_string())?;
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    )
    .map_err(|error| error.to_string())?;
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|error| error.to_string())?;
    Ok(response)
}

fn stop_child(child: &mut Child) {
    child.kill().ok();
    child.wait().ok();
}
