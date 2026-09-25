//! The workflow's process is killed after one step and again in the middle
//! of another, then run again. The order completes, and every step's effect
//! happened exactly once.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a failed expectation in an integration test is the test failing"
)]

use std::process::Command;

fn run(database: &std::path::Path, crash: Option<&str>) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_workflow-crash"));
    command.arg(database);
    match crash {
        Some(point) => command.env("CRASH", point),
        None => command.env_remove("CRASH"),
    };
    command.output().unwrap()
}

fn clean(database: &std::path::Path) {
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", database.display()));
    }
}

#[test]
fn a_killed_workflow_completes_with_each_step_once() {
    let database = std::env::temp_dir().join(format!("rustnative-crash-{}.db", std::process::id()));
    clean(&database);

    assert!(!run(&database, Some("after:reserve")).status.success(), "killed after reserving");
    assert!(
        !run(&database, Some("inside:charge")).status.success(),
        "killed while charging, before the charge committed"
    );
    let finished = run(&database, None);
    assert!(finished.status.success(), "{}", String::from_utf8_lossy(&finished.stderr));
    assert_eq!(String::from_utf8_lossy(&finished.stdout).trim(), "order 42 shipped");

    let connection = rusqlite::Connection::open(&database).unwrap();
    let count = |step: &str| -> i64 {
        connection
            .query_row("SELECT COUNT(*) FROM effects WHERE step = ?1", [step], |row| row.get(0))
            .unwrap()
    };
    assert_eq!(
        (count("reserve"), count("charge"), count("ship")),
        (1, 1, 1),
        "each step exactly once"
    );

    // Running again does nothing more.
    assert!(run(&database, None).status.success());
    assert_eq!(count("charge"), 1);
    drop(connection);
    clean(&database);
}
