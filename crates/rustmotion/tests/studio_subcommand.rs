use std::process::Command;

fn rustmotion() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rustmotion"))
}

fn top_level_help() -> String {
    let out = rustmotion().arg("--help").output().expect("run --help");
    assert!(out.status.success(), "`rustmotion --help` failed");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn lists_studio(help: &str) -> bool {
    help.lines()
        .any(|line| line.trim_start().starts_with("studio "))
}

#[cfg(feature = "studio")]
#[test]
fn studio_is_listed_when_the_feature_is_on() {
    let help = top_level_help();
    assert!(
        lists_studio(&help),
        "`studio` is missing from the subcommand list:\n{help}"
    );
}

#[cfg(feature = "studio")]
#[test]
fn studio_takes_the_same_flags_as_the_standalone_binary() {
    let out = rustmotion()
        .args(["studio", "--help"])
        .output()
        .expect("run studio --help");
    assert!(out.status.success(), "`rustmotion studio --help` failed");
    let help = String::from_utf8_lossy(&out.stdout);
    assert!(help.contains("--file"), "--file missing:\n{help}");
    assert!(help.contains("--dir"), "--dir missing:\n{help}");
}

#[cfg(not(feature = "studio"))]
#[test]
fn studio_stays_out_of_help_when_the_feature_is_off() {
    let help = top_level_help();
    assert!(
        !lists_studio(&help),
        "`studio` should not be advertised by a build that has no studio:\n{help}"
    );
}

#[cfg(not(feature = "studio"))]
#[test]
fn studio_names_the_feature_when_the_feature_is_off() {
    let out = rustmotion().arg("studio").output().expect("run studio");
    assert!(
        !out.status.success(),
        "`rustmotion studio` should fail in a build with no studio"
    );
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("--features studio"),
        "the error should say how to get the studio:\n{err}"
    );
    assert!(
        !err.contains("unrecognized subcommand"),
        "the subcommand should exist and explain itself:\n{err}"
    );
}
