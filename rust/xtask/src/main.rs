//! Repository tasks, run with `cargo xtask <task>` from anywhere in `rust/`.
//!
//! `ci` is the single definition of what CI checks: GitHub Actions runs
//! `cargo xtask ci`, the pre-push hook runs it, and so can anyone before
//! pushing. Plain Rust with no dependencies, so it behaves the same on
//! Linux, macOS and Windows.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Instant;

const USAGE: &str = "\
Usage: cargo xtask <task>

Tasks:
  ci              rustfmt --check, clippy -D warnings, the full test suite,
                  rustdoc -D warnings (broken doc links fail), then
                  cargo deny check advisories when cargo-deny is installed
  install-hooks   install a git pre-push hook that runs `cargo xtask ci`
  uninstall-hooks remove that hook";

/// One CI step: a label, the cargo arguments that run it, any
/// environment it needs, and an optional cargo subcommand it depends on
/// (with what it checks), skipped when that isn't installed.
struct Step {
    label: &'static str,
    args: &'static [&'static str],
    env: &'static [(&'static str, &'static str)],
    requires: Option<(&'static str, &'static str)>,
}

const CI_STEPS: &[Step] = &[
    Step {
        label: "rustfmt",
        args: &["fmt", "--all", "--check"],
        env: &[],
        requires: None,
    },
    Step {
        label: "clippy",
        args: &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--locked",
            "--",
            "-D",
            "warnings",
        ],
        env: &[],
        requires: None,
    },
    Step {
        label: "tests",
        args: &["test", "--workspace", "--locked"],
        env: &[],
        requires: None,
    },
    // Fails on a broken intra-doc link, so a role file's links to the
    // night regions that resolve it can't silently go stale.
    Step {
        label: "docs",
        args: &[
            "doc",
            "--workspace",
            "--no-deps",
            "--document-private-items",
            "--locked",
        ],
        // Private items are documented, so links to them are fine.
        env: &[("RUSTDOCFLAGS", "-D warnings -A rustdoc::private_intra_doc_links")],
        requires: None,
    },
    // Known vulnerabilities in the dependency tree (policy in deny.toml).
    // Optional locally; CI installs cargo-deny so it always runs there.
    Step {
        label: "deny",
        args: &["deny", "check", "advisories"],
        env: &[],
        requires: Some(("cargo-deny", "check advisories")),
    },
];

/// Marks a hook as ours, so uninstalling never deletes someone's own hook.
const HOOK_MARKER: &str = "# installed by cargo xtask install-hooks";

fn main() -> ExitCode {
    let task = env::args().nth(1);
    let result = match task.as_deref() {
        Some("ci") => ci(),
        Some("install-hooks") => install_hooks(),
        Some("uninstall-hooks") => uninstall_hooks(),
        Some("-h" | "--help" | "help") => {
            println!("{USAGE}");
            Ok(())
        }
        Some(other) => Err(format!("unknown task `{other}`\n\n{USAGE}")),
        None => Err(USAGE.to_string()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn ci() -> Result<(), String> {
    let workspace = workspace_root();
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let started = Instant::now();
    let mut skipped = vec![];
    for (i, step) in CI_STEPS.iter().enumerate() {
        if let Some((tool, purpose)) = step.requires {
            if !installed(tool) {
                let line = format!("==> {} skipped: install {tool} to {purpose}", step.label);
                println!("{line}");
                skipped.push(line);
                continue;
            }
        }
        println!(
            "==> [{}/{}] {}: cargo {}",
            i + 1,
            CI_STEPS.len(),
            step.label,
            step.args.join(" ")
        );
        let step_started = Instant::now();
        let status = Command::new(&cargo)
            .args(step.args)
            .envs(step.env.iter().copied())
            .current_dir(&workspace)
            .status()
            .map_err(|e| format!("could not run cargo: {e}"))?;
        if !status.success() {
            let hint = match step.label {
                "rustfmt" => "\nRun `cargo fmt --all` to fix the formatting.",
                _ => "",
            };
            return Err(format!("==> {} failed ({status}).{hint}", step.label));
        }
        println!(
            "==> {} passed in {:.1}s",
            step.label,
            step_started.elapsed().as_secs_f64()
        );
    }
    println!("==> CI passed in {:.1}s", started.elapsed().as_secs_f64());
    // Repeated so a green run says what it did not check.
    for line in skipped {
        println!("{line}");
    }
    Ok(())
}

/// Whether `tool` (a cargo subcommand binary such as `cargo-deny`) is on
/// PATH.
fn installed(tool: &str) -> bool {
    Command::new(tool)
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn install_hooks() -> Result<(), String> {
    let hook = pre_push_hook_path()?;
    if hook.exists() && !is_ours(&hook) {
        return Err(format!(
            "{} already exists and was not installed by xtask; leaving it alone.",
            hook.display()
        ));
    }
    fs::create_dir_all(hook.parent().expect("hook path has a parent")).map_err(|e| e.to_string())?;
    // Git runs hooks with sh on every platform (Git for Windows ships one).
    let script = format!(
        "#!/bin/sh\n{HOOK_MARKER}\n\
         # Runs the CI checks before every push. Skip once with `git push --no-verify`.\n\
         cd \"$(git rev-parse --show-toplevel)/rust\" || exit 1\n\
         exec cargo xtask ci\n"
    );
    fs::write(&hook, script).map_err(|e| format!("could not write {}: {e}", hook.display()))?;
    make_executable(&hook)?;
    println!(
        "Installed {}: every `git push` now runs `cargo xtask ci` first.",
        hook.display()
    );
    Ok(())
}

fn uninstall_hooks() -> Result<(), String> {
    let hook = pre_push_hook_path()?;
    if !hook.exists() {
        println!("No pre-push hook installed.");
    } else if is_ours(&hook) {
        fs::remove_file(&hook).map_err(|e| e.to_string())?;
        println!("Removed {}.", hook.display());
    } else {
        return Err(format!(
            "{} was not installed by xtask; leaving it alone.",
            hook.display()
        ));
    }
    Ok(())
}

/// The workspace root (`rust/`), the parent of this crate's directory.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives inside the workspace")
        .to_path_buf()
}

/// Asks git where hooks go, which respects `core.hooksPath` and worktrees.
fn pre_push_hook_path() -> Result<PathBuf, String> {
    let output = Command::new("git")
        .args(["rev-parse", "--git-path", "hooks"])
        .current_dir(workspace_root())
        .output()
        .map_err(|e| format!("could not run git: {e}"))?;
    if !output.status.success() {
        return Err("not inside a git checkout".to_string());
    }
    let hooks = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    let hooks = if hooks.is_absolute() {
        hooks
    } else {
        workspace_root().join(hooks)
    };
    // Tidier to print, and git may not have created hooks/ yet.
    let hooks = hooks.canonicalize().unwrap_or(hooks);
    Ok(hooks.join("pre-push"))
}

fn is_ours(hook: &Path) -> bool {
    fs::read_to_string(hook).is_ok_and(|s| s.contains(HOOK_MARKER))
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())
}

#[cfg(not(unix))]
fn make_executable(_: &Path) -> Result<(), String> {
    Ok(())
}
