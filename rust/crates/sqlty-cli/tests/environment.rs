//! `sqlty_cli::native::Environment`: the Python environment sqlty is installed in or run from.

#![expect(clippy::unwrap_used, reason = "a failed setup fails the test")]

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use sqlty_cli::native::Environment;

/// The `site-packages` of the environment at `prefix`, as `sys.path` lists it.
fn site_of(prefix: &Path) -> Option<PathBuf> {
    Environment::of_scripts(&prefix.join("bin"))
        .path
        .get(1)
        .cloned()
}

#[test]
fn reads_the_environment_it_is_installed_in() {
    let root = std::env::temp_dir().join(format!("sqlty-native-{}", std::process::id()));
    let site = root.join("lib/python3.13/site-packages");
    std::fs::create_dir_all(root.join("bin")).unwrap();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(&site).unwrap();
    std::fs::write(
        root.join("pyvenv.cfg"),
        "home = /usr\nversion_info = 3.14.2\n",
    )
    .unwrap();
    std::fs::write(
        site.join("a.pth"),
        "# comment\nimport x\n../../../src\nmissing\n",
    )
    .unwrap();
    let found = Environment::of_scripts(&root.join("bin"));
    assert_eq!(found.version, (3, 14));
    assert_eq!(found.path[..2], [root.join("bin"), site.clone()]);
    assert_eq!(found.path.len(), 3);
    assert!(found.path[2].ends_with("src"));
    std::fs::write(site.join("b.pth"), b"\xff").unwrap();
    std::fs::write(root.join("pyvenv.cfg"), "version = 3.12.1\n").unwrap();
    assert_eq!(Environment::of_scripts(&root.join("bin")).version, (3, 12));
    std::fs::remove_file(root.join("pyvenv.cfg")).unwrap();
    assert_eq!(Environment::of_scripts(&root.join("bin")).path.len(), 3);
    assert_eq!(Environment::of_scripts(&root.join("bin")).version, (3, 13));
    std::fs::remove_dir_all(&root).unwrap();
    let bare = Environment::of_scripts(&root.join("bin"));
    assert_eq!(bare.path, [root.join("bin")]);
    assert_eq!(bare.version.0, 3);
}

#[test]
fn spells_site_packages_as_the_disk_does() {
    let root = std::env::temp_dir().join(format!("sqlty-lib-{}", std::process::id()));
    let windows = root.join("Lib").join("site-packages");
    std::fs::create_dir_all(&windows).unwrap();
    assert_eq!(site_of(&root), Some(windows));
    std::fs::remove_dir_all(&root).unwrap();
    let posix = root.join("lib").join("python3.13").join("site-packages");
    std::fs::create_dir_all(&posix).unwrap();
    assert_eq!(site_of(&root), Some(posix));
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn finds_the_environment_of_the_project_outside_one() {
    let root = std::env::temp_dir().join(format!("sqlty-find-{}", std::process::id()));
    let venv = |prefix: &Path, scripts: &str| {
        std::fs::create_dir_all(prefix.join(scripts)).unwrap();
        std::fs::write(prefix.join("pyvenv.cfg"), "version_info = 3.15.0\n").unwrap();
        prefix.join(scripts)
    };
    let project = root.join("project");
    let cwd = project.join("app/db");
    std::fs::create_dir_all(&cwd).unwrap();
    let npm = root.join("node_modules/sqlty-linux-x64/bin/sqlty");
    let installed = venv(&root.join("installed"), "bin").join("sqlty");
    let local = venv(&project.join(".venv"), "Scripts");
    let active = venv(&root.join("active"), "bin");
    let conda = venv(&root.join("conda"), "bin");
    let none = |_: &str| None;
    let find = |executable: &Path, vars: &[(&str, &Path)]| {
        let vars: Vec<(String, OsString)> = vars
            .iter()
            .map(|(name, value)| (name.to_string(), value.as_os_str().to_owned()))
            .collect();
        Environment::find(executable, &cwd, |name| {
            vars.iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.clone())
        })
        .path[0]
            .clone()
    };
    let base = Path::new("base");
    assert_eq!(
        find(&installed, &[("VIRTUAL_ENV", &root.join("active"))]),
        root.join("installed/bin")
    );
    assert_eq!(
        find(
            &npm,
            &[
                ("VIRTUAL_ENV", &root.join("active")),
                ("CONDA_PREFIX", &root.join("conda"))
            ]
        ),
        active
    );
    assert_eq!(find(&npm, &[("CONDA_PREFIX", &root.join("conda"))]), conda);
    assert_eq!(
        find(
            &npm,
            &[
                ("CONDA_PREFIX", &root.join("conda")),
                ("CONDA_DEFAULT_ENV", base)
            ]
        ),
        local
    );
    assert_eq!(Environment::find(&npm, &cwd, none).version, (3, 15));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let marker = root.join("ran");
        let python = local.join("python");
        std::fs::write(project.join(".venv/pyvenv.cfg"), "home = /usr\n").unwrap();
        std::fs::write(&python, format!("#!/bin/sh\ntouch {}\n", marker.display())).unwrap();
        std::fs::set_permissions(&python, std::fs::Permissions::from_mode(0o755)).unwrap();
        let found = Environment::find(&npm, &cwd, none);
        assert_eq!((found.interpreter, found.version), (python, (3, 14)));
        assert!(
            !marker.exists(),
            "ran the interpreter of the checkout's .venv"
        );
    }
    std::fs::remove_dir_all(project.join(".venv")).unwrap();
    assert_eq!(
        find(
            &npm,
            &[
                ("CONDA_PREFIX", &root.join("conda")),
                ("CONDA_DEFAULT_ENV", base)
            ]
        ),
        conda
    );
    assert_eq!(find(&npm, &[]), npm.parent().unwrap());
    std::fs::remove_dir_all(&root).unwrap();
}
