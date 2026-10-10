#![cfg(test)]

use crate::package_test::{ask_artifact_offline, ask_offline};

#[test]
fn the_script_answer_overrides_config() {
    let answer = ask_artifact_offline(
        super::WELCOME_EVENT,
        None,
        None,
        include_str!("../data/.generated/example-all.script.json"),
    )
    .unwrap();
    assert_eq!(answer, Some(1.0));
    assert!(super::should_welcome(answer, Some("false")));
    assert!(!super::should_welcome(Some(0.0), Some("true")));
}

#[test]
fn no_script_uses_config() {
    let answer = ask_offline(super::WELCOME_EVENT, None, None, &[]).unwrap();
    assert!(!super::should_welcome(answer, Some("false")));
    assert!(super::should_welcome(answer, Some("true")));
    assert!(super::should_welcome(answer, Some("invalid")));
}
