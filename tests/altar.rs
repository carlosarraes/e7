use std::fs;
use std::os::unix::fs::PermissionsExt;

use assert_cmd::Command;
use predicates::str::contains;

// Replay the device boundary while keeping the real runner and image matching.
// The second purchase completes after the first reward screenshot is taken.
#[test]
fn continues_when_the_second_reward_popup_is_late() {
    run_reward_scenario(false, "buys: 2 in", "confirm\nconfirm\n");
}

#[test]
fn stops_without_rebuying_when_the_reward_never_appears() {
    run_reward_scenario(true, "buys: 0 in", "confirm\n");
}

fn run_reward_scenario(missing: bool, summary: &str, purchases: &str) {
    let dir = tempfile::tempdir().unwrap();
    for (name, fixture) in [
        ("altar", "altar_screen"),
        ("modal", "altar_modal_max"),
        ("popup", "altar_popup"),
    ] {
        let crop = image::open(format!(
            "{}/tests/fixtures/{fixture}.png",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
        .to_rgb8();
        let mut screen = image::RgbImage::new(1920, 1080);
        image::imageops::replace(&mut screen, &crop, 100, 610);
        screen.save(dir.path().join(format!("{name}.png"))).unwrap();
    }
    let adb = dir.path().join("adb");
    fs::write(
        &adb,
        r#"#!/bin/sh
set -eu
if [ "${1:-}" = '-s' ]; then shift 2; fi
case "$*" in
    devices) printf 'List of devices attached\ntablet\tdevice\n' ;;
    'shell wm size') printf 'Physical size: 1080x1920\n' ;;
    'exec-out screencap -p')
        state=$(cat state)
        if [ "$state" = pending ]; then
            cat modal.png
            echo popup > state
        else
            cat "$state.png"
        fi
        ;;
    'shell input tap 298 805') echo modal > state ;;
    'shell input tap 1120 884')
        echo confirm >> purchases
        if [ "$MISSING_REWARD" = true ]; then
            echo modal > state
        elif [ "$(wc -l < purchases)" -eq 1 ]; then
            echo popup > state
        else
            echo pending > state
        fi
        ;;
    'shell input tap 958 792') echo altar > state ;;
    *) echo "unexpected adb command: $*" >&2; exit 1 ;;
esac
"#,
    )
    .unwrap();
    fs::set_permissions(&adb, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(dir.path().join("state"), "altar\n").unwrap();
    let path = std::env::join_paths(
        std::iter::once(dir.path().to_path_buf())
            .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();

    Command::cargo_bin("e7")
        .unwrap()
        .current_dir(dir.path())
        .env("PATH", path)
        .env("MISSING_REWARD", missing.to_string())
        .timeout(std::time::Duration::from_secs(30))
        .args(["altar", "--buys", "2", "--tap-sleep", "0"])
        .assert()
        .success()
        .stderr(contains(summary));
    assert_eq!(
        fs::read_to_string(dir.path().join("purchases")).unwrap(),
        purchases,
        "waiting for a reward must not repeat the purchase"
    );
}
