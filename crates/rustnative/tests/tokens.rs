//! `rustnative tokens import` (`PLAN.md` Milestone 48): a W3C Design Tokens
//! file becomes the style file's `@theme` block, host roles noted, and a
//! second import replaces the first.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "helper functions in an integration test are test code too: a failed expectation is the test failing"
)]

use std::process::Command;

const TOKENS: &str = r##"{
  "brand": { "$type": "color", "blue": { "$value": "#0f6cbd" } },
  "color": {
    "$type": "color",
    "accent": { "$value": "{brand.blue}", "$extensions": { "rustnative.host": "accent" } }
  },
  "radius": { "$type": "dimension", "card": { "$value": "8px" } },
  "shadow": { "$type": "shadow", "sm": { "$value": "0 1px 2px black" } }
}"##;

#[test]
fn a_token_file_becomes_the_theme_block() {
    let project = std::env::temp_dir().join(format!("rustnative-tokens-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&project);
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("rustnative.toml"),
        "[app]\nname = \"tokens\"\nid = \"dev.rustnative.tokens\"\ndisplay-name = \"Tokens\"\n\
         version = \"0.1.0\"\npublisher = \"CN=Rust Native\"\ndescription = \"A token import\"\n",
    )
    .unwrap();
    std::fs::write(project.join("app.css"), "@utility headline {\n  @apply font-bold;\n}\n")
        .unwrap();
    std::fs::write(project.join("tokens.json"), TOKENS).unwrap();

    let run = || {
        Command::new(env!("CARGO_BIN_EXE_rustnative"))
            .current_dir(&project)
            .args(["tokens", "import", "tokens.json"])
            .output()
            .unwrap()
    };
    let output = run();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains("--color-accent follows the host's accent")
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("shadow"),
        "the skipped token is reported"
    );

    let style = std::fs::read_to_string(project.join("app.css")).unwrap();
    assert!(style.contains("--color-brand-blue: #0f6cbd;"), "{style}");
    assert!(
        style.contains("--color-accent: var(--color-brand-blue); /* host: accent */"),
        "{style}"
    );
    assert!(style.contains("--radius-card: 8px;"), "{style}");
    assert!(style.contains("@utility headline"), "the rest of the file is kept");

    assert!(run().status.success());
    let again = std::fs::read_to_string(project.join("app.css")).unwrap();
    assert_eq!(again.matches("tokens:begin").count(), 1, "replaced, not appended");
    let _ = std::fs::remove_dir_all(project);
}
