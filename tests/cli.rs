use assert_cmd::Command;
use predicates::prelude::*;
use predicates::str::contains;

#[test]
fn help_lists_subcommands() {
    Command::cargo_bin("e7")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("run").and(contains("devices")).and(contains("screenshot")));
}

#[test]
fn run_requires_a_limit() {
    Command::cargo_bin("e7")
        .unwrap()
        .arg("run")
        .assert()
        .failure()
        .stderr(contains("--refreshes").and(contains("--skystones")));
}
