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
  ci              rustfmt --check, clippy -D warnings, then the full test suite
  install-hooks   install a git pre-push hook that runs `cargo xtask ci`
  uninstall-hooks remove that hook";

/// One CI step: a label and the cargo arguments that run it.
struct Step {
    label: &'static str,
    args: &'static [&'static str],
}

const CI_STEPS: &[Step] = &[
    Step {
        label: "rustfmt",
        args: &["fmt", "--all", "--check"],
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
    },
    Step {
        label: "tests",
        args: &["test", "--workspace", "--locked"],
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
    for (i, step) in CI_STEPS.iter().enumerate() {
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
    Ok(())
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
