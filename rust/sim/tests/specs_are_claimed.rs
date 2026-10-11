//! Every Scenario in the .feature files names what checks it.
//!
//! The .feature files (every `features/` directory under `rust/`: the
//! workspace-wide `rust/features` and each crate's `<crate>/features`) are
//! the approval documents written before the code. Directly under each
//! `Scenario:` or `Scenario Outline:` line comes one of
//!
//! ```text
//! # covered by: <test fn>[, <test fn>...]
//! # covered by: engine_transcripts
//! # not tested: <short reason>
//! ```
//!
//! `engine_transcripts` means the random-game digest gate
//! (`tests/engine_transcripts.rs`) exercises the behaviour with no direct
//! test. Every other name must be a `fn` somewhere under `rust/`.

use std::fs;
use std::path::{Path, PathBuf};

fn rust_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("sim lives inside the workspace")
        .to_path_buf()
}

/// Every file under `dir` with extension `ext`, skipping `target`.
fn files(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n != "target" && n != ".git") {
                files(&path, ext, out);
            }
        } else if path.extension().is_some_and(|e| e == ext) {
            out.push(path);
        }
    }
}

/// Every directory named `features` under `dir`, skipping `target`.
fn feature_dirs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name() else { continue };
        if !path.is_dir() || name == "target" || name == ".git" {
            continue;
        }
        if name == "features" {
            out.push(path);
        } else {
            feature_dirs(&path, out);
        }
    }
}

#[test]
fn every_scenario_is_claimed() {
    let root = rust_root();
    let mut sources = vec![];
    files(&root, "rs", &mut sources);
    let code: String = sources
        .iter()
        .map(|p| fs::read_to_string(p).unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n");

    let mut dirs = vec![];
    feature_dirs(&root, &mut dirs);
    let mut features = vec![];
    for dir in &dirs {
        files(dir, "feature", &mut features);
    }
    features.sort();
    assert!(!features.is_empty(), "no .feature files found under {}", root.display());

    let mut problems = vec![];
    let mut scenarios = 0;
    for path in &features {
        let text = fs::read_to_string(path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        let name = path.strip_prefix(&root).unwrap_or(path).display().to_string();
        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim_start();
            if !(trimmed.starts_with("Scenario:") || trimmed.starts_with("Scenario Outline:")) {
                continue;
            }
            scenarios += 1;
            let at = format!("{name}:{}: {}", i + 1, trimmed);
            let next = lines[i + 1..].iter().map(|l| l.trim()).find(|l| !l.is_empty());
            match next {
                Some(n) if n.starts_with("# not tested:") && n.len() > "# not tested:".len() => {}
                Some(n) if n.starts_with("# covered by:") => {
                    let names: Vec<&str> = n["# covered by:".len()..].split(',').map(str::trim).collect();
                    if names.iter().any(|n| n.is_empty()) {
                        problems.push(format!("{at}\n    empty name in `{n}`"));
                    }
                    for test in names.iter().filter(|t| !t.is_empty() && **t != "engine_transcripts") {
                        if !code.contains(&format!("fn {test}(")) {
                            problems.push(format!("{at}\n    no `fn {test}(` under rust/"));
                        }
                    }
                }
                _ => problems.push(format!(
                    "{at}\n    the next line must be `# covered by: ...` or `# not tested: ...`"
                )),
            }
        }
    }
    assert!(scenarios > 0, "no scenarios found");
    assert!(
        problems.is_empty(),
        "{} of {scenarios} scenarios are not claimed:\n{}",
        problems.len(),
        problems.join("\n")
    );
}
