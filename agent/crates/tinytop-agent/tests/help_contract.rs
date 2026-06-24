use std::process::Command;

#[test]
fn help_lists_writer_server_command() {
    let output = Command::new(env!("CARGO_BIN_EXE_tinytop-agent"))
        .arg("help")
        .output()
        .expect("tinytop-agent help should run");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("help output should be UTF-8");
    assert!(stdout.contains("tinytop-agent serve"));
    assert!(stdout.contains("--host"));
    assert!(stdout.contains("--port"));
    assert!(stdout.contains("--sqlite"));
    assert!(stdout.contains("tinytop-agent db stats"));
}
