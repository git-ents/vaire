//! CLI edge cases through the binary: help wording and documented examples,
//! `--color` control and machine-readable purity, exit codes, and robustness
//! against malformed, oversized, duplicated, and awkwardly named inputs.
#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "tests use unwraps, direct indexing, and panics to express failed expectations"
)]

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_vaire");

/// One valid, fully attributed leaf requirement.
const VALID: &str = "[requirement#SWR-0001]\n[modality=shall, kind=functional, status=draft, verification=test]\n--\nThe system shall boot within 5 s.\n--\n";

/// [`VALID`] with a different id, so two files form a clean set.
const VALID_ALT: &str = "[requirement#SWR-0002]\n[modality=shall, kind=functional, status=draft, verification=test]\n--\nThe system shall log every fault.\n--\n";

/// Prose modality disagrees with the attribute: fails V4 and nothing else.
const VIOLATION: &str = "[requirement#SWR-0103]\n[modality=shall, kind=functional, verification=test]\n--\nThe system may retry dropped samples.\n--\n";

/// Two valid records, for duplicate-argument accounting.
const TWO: &str = "[requirement#SWR-0001]\n[modality=shall, kind=functional, verification=test]\n--\nThe system shall boot.\n--\n\n[requirement#SWR-0002]\n[modality=shall, kind=functional, verification=test]\n--\nThe system shall log.\n--\n";

/// Run the binary with a controlled environment: color-influencing variables
/// the harness may carry are stripped unless `envs` re-adds them.
fn run_with(args: &[&str], envs: &[(&str, &str)]) -> Output {
    let mut command = Command::new(BIN);
    command.args(args);
    for var in ["NO_COLOR", "CLICOLOR", "CLICOLOR_FORCE"] {
        command.env_remove(var);
    }
    for (key, value) in envs {
        command.env(key, value);
    }
    command.output().unwrap()
}

fn run(args: &[&str]) -> Output {
    run_with(args, &[])
}

fn stdout_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// Collapse all whitespace runs, so assertions survive help-text wrapping.
fn squished(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn temp_source(dir: &Path, name: &str, contents: &str) -> String {
    let path = dir.join(name);
    fs::write(&path, contents).unwrap();
    path.to_str().unwrap().to_owned()
}

fn temp_source_bytes(dir: &Path, name: &str, contents: &[u8]) -> String {
    let path = dir.join(name);
    fs::write(&path, contents).unwrap();
    path.to_str().unwrap().to_owned()
}

/// `extract -c` writes one JSON document, so its help must say so.
#[test]
fn extract_compact_help_describes_one_document() {
    let help = squished(&stdout_of(&run(&["extract", "--help"])));
    assert!(
        help.contains("one compact single-line JSON document"),
        "{help}"
    );
    assert!(!help.contains("per line"), "{help}");
}

#[test]
fn root_help_lists_the_documented_examples() {
    let help = squished(&stdout_of(&run(&["--help"])));
    assert!(help.contains("Examples:"), "{help}");
    for example in [
        "vaire extract spec.adoc > records.json",
        "vaire extract --compact spec.adoc > records.json",
        "vaire check spec.adoc derived.adoc",
        "vaire list spec.adoc --modality shall --status draft",
        "vaire show spec.adoc SWR-0001",
        "vaire emit -n records.json spec.adoc",
        "vaire emit --diff records.json spec.adoc",
        "vaire edit -n spec.adoc SWR-0001 --set status=approved",
    ] {
        assert!(help.contains(example), "missing {example:?} in:\n{help}");
    }
}

/// The flag works in either position because it is a global argument.
#[test]
fn color_always_colors_piped_human_output() {
    let dir = tempfile::tempdir().unwrap();
    let bad = temp_source(dir.path(), "bad.adoc", VIOLATION);
    let good = temp_source(dir.path(), "good.adoc", VALID);

    let after = stderr_of(&run(&["check", &bad, "--color", "always"]));
    assert!(after.contains('\x1b'), "{after:?}");
    assert_eq!(
        run(&["check", &bad, "--color", "always"]).status.code(),
        Some(1)
    );

    let before = stderr_of(&run(&["--color", "always", "check", &bad]));
    assert!(before.contains('\x1b'), "{before:?}");

    let table = stdout_of(&run(&["list", &good, "--color", "always"]));
    assert!(table.contains('\x1b'), "{table:?}");
}

#[test]
fn color_never_and_the_piped_default_are_plain() {
    let dir = tempfile::tempdir().unwrap();
    let bad = temp_source(dir.path(), "bad.adoc", VIOLATION);
    let good = temp_source(dir.path(), "good.adoc", VALID);

    let never = stderr_of(&run(&["check", &bad, "--color", "never"]));
    assert!(!never.contains('\x1b'), "{never:?}");
    let piped = stderr_of(&run(&["check", &bad]));
    assert!(!piped.contains('\x1b'), "{piped:?}");

    let table = stdout_of(&run(&["list", &good]));
    assert!(!table.contains('\x1b'), "{table:?}");
    let table_never = stdout_of(&run(&["list", &good, "--color", "never"]));
    assert!(!table_never.contains('\x1b'), "{table_never:?}");
}

#[test]
fn machine_readable_output_never_carries_ansi() {
    let dir = tempfile::tempdir().unwrap();
    let bad = temp_source(dir.path(), "bad.adoc", VIOLATION);
    let good = temp_source(dir.path(), "good.adoc", VALID);
    let cases: [(&[&str], bool); 6] = [
        (&["extract", &good, "--color", "always"], true),
        (
            &["check", &bad, "--color", "always", "--format", "json"],
            false,
        ),
        (
            &["check", &bad, "--color", "always", "--format", "compact"],
            false,
        ),
        (
            &["list", &good, "--color", "always", "--format", "json"],
            true,
        ),
        (
            &["list", &good, "--color", "always", "--format", "compact"],
            true,
        ),
        (
            &["list", &good, "--color", "always", "--format", "tsv"],
            true,
        ),
    ];
    for (args, succeeds) in cases {
        let out = run(args);
        assert_eq!(
            out.status.code(),
            Some(if succeeds { 0 } else { 1 }),
            "{args:?}: {}",
            stderr_of(&out)
        );
        assert!(
            !out.stdout.contains(&0x1b),
            "{args:?}: ESC in {:?}",
            stdout_of(&out)
        );
    }
}

/// `NO_COLOR` wins over `CLICOLOR_FORCE` while `--color` is `auto`, and an
/// explicit `--color always` overrides `NO_COLOR`.
#[test]
fn no_color_env_is_respected_in_auto_mode() {
    let dir = tempfile::tempdir().unwrap();
    let bad = temp_source(dir.path(), "bad.adoc", VIOLATION);

    let auto = run_with(
        &["check", &bad],
        &[("NO_COLOR", "1"), ("CLICOLOR_FORCE", "1")],
    );
    assert!(!stderr_of(&auto).contains('\x1b'), "{:?}", stderr_of(&auto));

    let forced = run_with(&["check", &bad, "--color", "always"], &[("NO_COLOR", "1")]);
    assert!(
        stderr_of(&forced).contains('\x1b'),
        "{:?}",
        stderr_of(&forced)
    );
}

#[test]
fn missing_files_error_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let ghost = dir.path().join("ghost.adoc").to_str().unwrap().to_owned();
    for args in [
        vec!["extract", &ghost],
        vec!["check", &ghost],
        vec!["list", &ghost],
        vec!["show", &ghost, "SWR-0001"],
        vec!["emit", &ghost, &ghost],
        vec!["edit", &ghost, "SWR-0001", "--set", "status=active"],
    ] {
        let out = run(&args);
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert!(out.stdout.is_empty(), "{args:?}");
        assert!(
            stderr_of(&out).starts_with("vaire: "),
            "{args:?}: {:?}",
            stderr_of(&out)
        );
    }
}

#[test]
fn directories_passed_as_files_error_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let nested = dir.path().join("reqs");
    fs::create_dir(&nested).unwrap();
    let nested = nested.to_str().unwrap().to_owned();
    for args in [
        vec!["extract", &nested],
        vec!["check", &nested],
        vec!["list", &nested],
        vec!["show", &nested, "SWR-0001"],
    ] {
        let out = run(&args);
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert!(out.stdout.is_empty(), "{args:?}");
        assert!(
            stderr_of(&out).starts_with("vaire: "),
            "{args:?}: {:?}",
            stderr_of(&out)
        );
    }
}

#[test]
fn invalid_utf8_errors_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let binary = temp_source_bytes(dir.path(), "binary.adoc", &[0xFF, 0xFE, 0xBA, 0xAD]);
    for args in [
        vec!["extract", &binary],
        vec!["check", &binary],
        vec!["list", &binary],
        vec!["show", &binary, "SWR-0001"],
    ] {
        let out = run(&args);
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert!(out.stdout.is_empty(), "{args:?}");
        let stderr = stderr_of(&out);
        assert!(stderr.starts_with("vaire: "), "{args:?}: {stderr:?}");
        assert!(stderr.contains("UTF-8"), "{args:?}: {stderr:?}");
    }
}

/// NULs are valid UTF-8, so they flow through extraction: attribute-line
/// garbage yields no records, and a NUL inside a body stays in the record.
#[test]
fn nul_bytes_never_panic() {
    let dir = tempfile::tempdir().unwrap();
    let garbage = temp_source_bytes(dir.path(), "garbage.adoc", b"garbage ]][\n\x00\x00 more\n");
    let out = run(&["extract", &garbage]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr_of(&out));
    assert_eq!(stdout_of(&out).trim_end(), "[]");

    let body = "[requirement#A-1]\n[modality=shall, kind=functional, verification=test]\n--\nThe system shall boot.\x00\n--\n";
    let nul = temp_source_bytes(dir.path(), "nul.adoc", body.as_bytes());
    let out = run(&["extract", &nul]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr_of(&out));
    let records: Vec<Value> = serde_json::from_str(stdout_of(&out).trim()).unwrap();
    assert_eq!(records.len(), 1);

    let out = run(&["check", &nul]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr_of(&out));
}

#[test]
fn empty_files_are_handled() {
    let dir = tempfile::tempdir().unwrap();
    let empty = temp_source(dir.path(), "empty.adoc", "");
    let out = run(&["extract", &empty]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr_of(&out));
    assert_eq!(stdout_of(&out).trim_end(), "[]");
    let out = run(&["check", &empty]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr_of(&out));
    let out = run(&["list", &empty]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr_of(&out));
    assert!(
        stdout_of(&out).contains("ID  KIND"),
        "{:?}",
        stdout_of(&out)
    );
    let out = run(&["show", &empty, "SWR-0001"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr_of(&out).contains("not found"),
        "{:?}",
        stderr_of(&out)
    );
}

/// ~1 MiB of prose around one requirement: extraction and validation
/// complete successfully on oversized input.
#[test]
fn a_megabyte_of_prose_is_extracted_and_checked() {
    let dir = tempfile::tempdir().unwrap();
    let filler = "x".repeat(1024 * 1024 - VALID.len() - 2);
    let big = temp_source(dir.path(), "big.adoc", &format!("{VALID}\n{filler}\n"));
    let out = run(&["extract", &big]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr_of(&out));
    let records: Vec<Value> = serde_json::from_str(stdout_of(&out).trim()).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["id"], "SWR-0001");
    let out = run(&["check", &big]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr_of(&out));
}

/// Documented dedupe policy: a path repeated on the command line is read
/// once, so read-only commands neither duplicate records nor report phantom
/// cross-file violations.
#[test]
fn duplicate_file_arguments_are_read_once() {
    let dir = tempfile::tempdir().unwrap();
    let source = temp_source(dir.path(), "two.adoc", TWO);

    let out = run(&["extract", &source, &source]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr_of(&out));
    let records: Vec<Value> = serde_json::from_str(stdout_of(&out).trim()).unwrap();
    assert_eq!(records.len(), 2, "not 4: repeated path is read once");

    let out = run(&["check", &source, &source]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr_of(&out));

    let stdout = stdout_of(&run(&["list", &source, &source]));
    let section = format!("--- {source}");
    assert_eq!(
        stdout
            .lines()
            .filter(|line| *line == section.as_str())
            .count(),
        1,
        "{stdout}"
    );
}

#[test]
fn paths_with_spaces_and_unicode_work() {
    let dir = tempfile::tempdir().unwrap();
    let spaced = temp_source(dir.path(), "my reqs.adoc", VALID);
    let unicode = temp_source(dir.path(), "déjà vu.adoc", VALID_ALT);

    let out = run(&["extract", &spaced, &unicode]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr_of(&out));
    let records: Vec<Value> = serde_json::from_str(stdout_of(&out).trim()).unwrap();
    let files: Vec<&str> = records
        .iter()
        .map(|record| record["file"].as_str().unwrap())
        .collect();
    assert_eq!(files, [spaced.as_str(), unicode.as_str()]);

    let out = run(&["check", &spaced, &unicode]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr_of(&out));

    let out = run(&["show", &unicode, "SWR-0002"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr_of(&out));
    assert!(
        stdout_of(&out).contains("id: SWR-0002"),
        "{:?}",
        stdout_of(&out)
    );
}

/// A failed `check` followed by failed `edit` calls never writes the source.
#[test]
fn failed_check_then_failed_edit_leave_the_source_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let source = temp_source(dir.path(), "conflict.adoc", VIOLATION);
    let original = fs::read(&source).unwrap();

    let out = run(&["check", &source]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr_of(&out));
    assert_eq!(fs::read(&source).unwrap(), original);

    for args in [
        vec!["edit", &source, "SWR-0103", "--set", "flavor=umami"],
        vec!["edit", &source, "SWR-9999", "--set", "status=active"],
    ] {
        let out = run(&args);
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert!(out.stdout.is_empty(), "{args:?}");
        assert_eq!(fs::read(&source).unwrap(), original, "{args:?} wrote");
    }
}

/// Permission-denied reads report the usual operational error; skipped when
/// the test can read files it has no permission for (running as root).
#[test]
#[cfg(unix)]
fn permission_denied_errors_cleanly() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let locked = temp_source(dir.path(), "locked.adoc", VALID);
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    let out = run(&["extract", &locked]);
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o644)).unwrap();

    if out.status.success() {
        return;
    }
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
    assert!(
        stderr_of(&out).starts_with("vaire: "),
        "{:?}",
        stderr_of(&out)
    );
}

/// clap usage errors exit 2, distinct from operational errors (1); a typo'd
/// subcommand gets the real one suggested. The typo is cut from `extract` at
/// runtime so the typos pre-commit hook does not correct it away.
#[test]
fn usage_errors_exit_two() {
    let typo = "extract".strip_suffix('t').unwrap();
    let dir = tempfile::tempdir().unwrap();
    let good = temp_source(dir.path(), "good.adoc", VALID);
    for args in [
        vec!["extract"],
        vec!["check"],
        vec!["list"],
        vec!["--bogus"],
        vec![typo],
        vec!["list", "--modality", "maybe", &good],
        vec!["--color", "bogus", "check", &good],
    ] {
        let out = run(&args);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {}", stderr_of(&out));
    }
    let tip = stderr_of(&run(&[typo]));
    assert!(tip.contains("extract"), "{tip}");
}

#[test]
fn success_including_noop_exits_zero() {
    let dir = tempfile::tempdir().unwrap();
    let source = temp_source(dir.path(), "spec.adoc", VALID);

    let out = run(&["check", &source]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr_of(&out));

    let records = stdout_of(&run(&["extract", &source]));
    let records = temp_source(dir.path(), "records.json", &records);
    let out = run(&["emit", "-n", &records, &source]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr_of(&out));
    assert!(
        stderr_of(&out).contains("no changes"),
        "{:?}",
        stderr_of(&out)
    );

    let out = run(&["edit", &source, "SWR-0001", "--set", "status=draft"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr_of(&out));
}

/// Every command from `--help`'s Examples block, run against real files with
/// the documented outcome.
#[test]
fn documented_examples_run() {
    let dir = tempfile::tempdir().unwrap();
    let spec = temp_source(dir.path(), "spec.adoc", VALID);
    let derived = temp_source(dir.path(), "derived.adoc", VALID_ALT);

    let records = stdout_of(&run(&["extract", &spec]));
    let parsed: Value = serde_json::from_str(records.trim()).unwrap();
    assert_eq!(parsed.as_array().unwrap().len(), 1);

    let compact = stdout_of(&run(&["extract", "--compact", &spec]));
    assert_eq!(compact.lines().count(), 1, "{compact:?}");
    serde_json::from_str::<Value>(compact.trim()).unwrap();

    let out = run(&["check", &spec, &derived]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr_of(&out));

    let table = stdout_of(&run(&[
        "list",
        &spec,
        "--modality",
        "shall",
        "--status",
        "draft",
    ]));
    assert!(table.contains("SWR-0001"), "{table}");

    let shown = stdout_of(&run(&["show", &spec, "SWR-0001"]));
    assert!(shown.contains("id: SWR-0001"), "{shown}");

    let records_file = temp_source(dir.path(), "records.json", &records);
    let out = run(&["emit", "-n", &records_file, &spec]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr_of(&out));
    assert!(
        stderr_of(&out).contains("no changes"),
        "{:?}",
        stderr_of(&out)
    );

    let mut edited = parsed.clone();
    for record in edited.as_array_mut().unwrap() {
        for line in record["attr_lines"].as_array_mut().unwrap() {
            for item in line["items"].as_array_mut().unwrap() {
                if let Some(kv) = item.get_mut("kv")
                    && kv["name"] == "status"
                {
                    kv["value"] = Value::String("approved".to_owned());
                }
            }
        }
    }
    let edited_file = temp_source(
        dir.path(),
        "approved.json",
        &serde_json::to_string(&edited).unwrap(),
    );
    let out = run(&["emit", "--diff", &edited_file, &spec]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr_of(&out));
    let diff = stderr_of(&out);
    assert!(diff.contains("- [modality=shall"), "{diff:?}");
    assert!(diff.contains("+ [modality=shall"), "{diff:?}");
    assert!(
        fs::read_to_string(&spec)
            .unwrap()
            .contains("status=approved"),
        "emit --diff writes the file"
    );

    let out = run(&[
        "edit",
        "-n",
        &derived,
        "SWR-0002",
        "--set",
        "status=approved",
    ]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr_of(&out));
    assert!(
        fs::read_to_string(&derived)
            .unwrap()
            .contains("status=draft"),
        "edit -n writes nothing"
    );
    let out = run(&["edit", &derived, "SWR-0002", "--set", "status=approved"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr_of(&out));
    assert!(
        fs::read_to_string(&derived)
            .unwrap()
            .contains("status=approved"),
        "edit without -n applies"
    );
}
