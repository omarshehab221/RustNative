//! `rustnative db` (`PLAN.md` Milestone 49): the next migration from the
//! model with a confirmed rename, applied with a dry run first, reversed.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "helper functions in an integration test are test code too: a failed expectation is the test failing"
)]

use std::process::Command;

#[test]
fn migrations_come_from_the_model() {
    let project = std::env::temp_dir().join(format!("rustnative-db-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&project);
    std::fs::create_dir_all(project.join("migrations")).unwrap();
    std::fs::write(
        project.join("rustnative.toml"),
        "[app]\nname = \"db\"\nid = \"dev.rustnative.db\"\ndisplay-name = \"Db\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    std::fs::write(
        project.join("migrations/0001_users.up.sql"),
        "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);",
    )
    .unwrap();
    std::fs::write(project.join("migrations/0001_users.down.sql"), "DROP TABLE users;").unwrap();
    std::fs::write(
        project.join("schema.toml"),
        "[tables.users]\ncolumns = [\n  { name = \"id\", type = \"INTEGER\", primary_key = true },\n  \
         { name = \"display_name\", type = \"TEXT\", not_null = true, default = \"''\" },\n]\n",
    )
    .unwrap();

    let run = |arguments: &[&str]| {
        let output = Command::new(env!("CARGO_BIN_EXE_rustnative"))
            .current_dir(&project)
            .args(arguments)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{arguments:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    };
    run(&["db", "diff", "rename_name", "--rename", "users.name:display_name"]);
    let up = std::fs::read_to_string(project.join("migrations/0002_rename_name.up.sql")).unwrap();
    assert_eq!(up, "ALTER TABLE users RENAME COLUMN name TO display_name;\n");
    assert!(run(&["db", "diff", "again", "--no-prompt"]).contains("nothing to write"));

    let dry = run(&["db", "migrate", "app.db", "--dry-run"]);
    assert!(dry.contains("would apply 0001_users") && dry.contains("would apply 0002_rename_name"));
    assert!(run(&["db", "migrate", "app.db"]).contains("applied 0002_rename_name"));
    assert!(
        run(&["db", "rollback", "app.db", "--to", "0001_users"])
            .contains("reversed 0002_rename_name")
    );
    let _ = std::fs::remove_dir_all(project);
}
