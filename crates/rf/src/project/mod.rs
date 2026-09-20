//! Creating a project, and finding the one you are standing in.

mod templates;

use std::path::{Path, PathBuf};

use crate::config::{self, Config};
use crate::error::{Error, Result};

/// Where a project is and what it says about itself.
#[derive(Debug, Clone)]
pub struct Project {
    /// The folder holding `rf.toml`.
    pub root: PathBuf,
    /// What `rf.toml` says.
    pub config: Config,
}

impl Project {
    /// Finds the project `directory` is in: the nearest ancestor with an
    /// `rf.toml`, starting with `directory` itself.
    ///
    /// # Errors
    ///
    /// [`Error::Usage`] when there is no project, or the config's error
    /// when there is one and it cannot be used.
    pub fn find(directory: &Path) -> Result<Self> {
        let mut candidate = Some(directory);
        while let Some(root) = candidate {
            if root.join(config::FILE_NAME).is_file() {
                return Ok(Self { root: root.to_path_buf(), config: Config::load(root)? });
            }
            candidate = root.parent();
        }
        Err(Error::Usage(format!(
            "no {} here or in any parent folder — run `rf new <name>` to make a project",
            config::FILE_NAME
        )))
    }
}

/// How a generated project depends on the framework.
#[derive(Debug, Clone)]
pub enum FrameworkSource {
    /// Published versions from crates.io.
    Published(String),
    /// A checkout of this workspace, by path — what the framework's own
    /// tests and anyone working on the framework itself use.
    Path(PathBuf),
}

impl FrameworkSource {
    fn dependencies(&self) -> String {
        match self {
            Self::Published(version) => {
                format!("framework-core = \"{version}\"\nframework-windows = \"{version}\"\n")
            }
            Self::Path(path) => {
                let path = path.display().to_string().replace('\\', "/");
                format!(
                    "framework-core = {{ path = \"{path}/crates/framework-core\" }}\n\
                     framework-windows = {{ path = \"{path}/crates/framework-windows\" }}\n"
                )
            }
        }
    }
}

/// Creates a new project called `name` in `parent`, returning its folder.
///
/// # Errors
///
/// [`Error::Usage`] if the name is not usable or the folder already exists,
/// and [`Error::Io`] if anything could not be written.
pub fn create(parent: &Path, name: &str, framework: &FrameworkSource) -> Result<PathBuf> {
    let config = Config::template(name);
    config.validate().map_err(|error| match error {
        config::ConfigError::Invalid { field: "app.name", problem } => {
            Error::Usage(format!("`{name}` cannot be a project name: {problem}"))
        }
        other => Error::Config(other),
    })?;

    let root = parent.join(name);
    if root.exists() {
        return Err(Error::Usage(format!("{} already exists", root.display())));
    }
    write(&root.join("src"), "main.rs", &fill(templates::MAIN_RS, &config))?;
    write(
        &root,
        "Cargo.toml",
        &fill(templates::CARGO_TOML, &config)
            .replace("{{dependencies}}", &framework.dependencies()),
    )?;
    let rf_toml = toml::to_string_pretty(&config).map_err(|cause| Error::Io {
        what: "write rf.toml".to_owned(),
        cause: std::io::Error::other(cause.to_string()),
    })?;
    write(&root, config::FILE_NAME, &rf_toml)?;
    write(&root, ".gitignore", templates::GITIGNORE)?;
    write(&root, "README.md", &fill(templates::README, &config))?;
    Ok(root)
}

/// Substitutes a template's placeholders with what the config says.
fn fill(template: &str, config: &Config) -> String {
    template
        .replace("{{name}}", &config.app.name)
        .replace("{{display_name}}", &config.app.display_name)
        .replace("{{app_id}}", &config.app.id)
        .replace("{{version}}", &config.app.version)
}

fn write(directory: &Path, name: &str, contents: &str) -> Result<()> {
    std::fs::create_dir_all(directory)
        .map_err(|cause| Error::Io { what: format!("create {}", directory.display()), cause })?;
    let path = directory.join(name);
    std::fs::write(&path, contents)
        .map_err(|cause| Error::Io { what: format!("write {}", path.display()), cause })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let directory =
            std::env::temp_dir().join(format!("rf-project-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a scratch folder");
        directory
    }

    #[test]
    fn a_new_project_has_everything_it_needs_and_names_itself_consistently() {
        let parent = scratch("create");
        let root = create(&parent, "demo-app", &FrameworkSource::Published("0.1".to_owned()))
            .expect("created");
        for file in ["Cargo.toml", "rf.toml", ".gitignore", "README.md", "src/main.rs"] {
            assert!(root.join(file).is_file(), "{file} is generated");
        }
        let manifest = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
        assert!(manifest.contains("name = \"demo-app\""));
        assert!(manifest.contains("framework-windows = \"0.1\""));
        let main = std::fs::read_to_string(root.join("src/main.rs")).unwrap();
        assert!(main.contains("com.example.demoapp"), "the app id reaches the source");
        assert!(!main.contains("{{"), "every placeholder is filled: {main}");

        let project = Project::find(&root.join("src")).expect("found from inside the project");
        assert_eq!(project.config.app.name, "demo-app");
        assert_eq!(project.root, root);
    }

    #[test]
    fn a_path_dependency_is_written_for_a_checkout() {
        let parent = scratch("path-dep");
        let root = create(
            &parent,
            "linked",
            &FrameworkSource::Path(PathBuf::from("C:\\work\\RustNative")),
        )
        .expect("created");
        let manifest = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
        assert!(
            manifest.contains("path = \"C:/work/RustNative/crates/framework-core\""),
            "{manifest}"
        );
    }

    #[test]
    fn an_unusable_name_is_refused_before_anything_is_written() {
        let parent = scratch("bad-name");
        let error = create(&parent, "not a name", &FrameworkSource::Published("0.1".to_owned()))
            .expect_err("refused");
        assert_eq!(error.exit_code(), 2);
        assert!(!parent.join("not a name").exists());
    }

    #[test]
    fn creating_over_an_existing_folder_is_refused() {
        let parent = scratch("exists");
        std::fs::create_dir_all(parent.join("taken")).unwrap();
        let error = create(&parent, "taken", &FrameworkSource::Published("0.1".to_owned()))
            .expect_err("refused");
        assert!(error.to_string().contains("already exists"));
    }

    #[test]
    fn a_folder_with_no_project_says_so() {
        let parent = scratch("empty");
        let error = Project::find(&parent).expect_err("no project here");
        assert_eq!(error.exit_code(), 2);
        assert!(error.to_string().contains("rf new"));
    }
}
