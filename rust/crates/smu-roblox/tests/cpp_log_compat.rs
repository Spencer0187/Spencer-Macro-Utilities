#![allow(clippy::expect_used)]

use serde::Deserialize;
use smu_roblox::{RobloxLogReader, RobloxLogSnapshot};
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

const LOG_FIXTURE: &str = include_str!("../../../../compat/roblox/log_cases.json");

#[derive(Debug, Deserialize)]
struct LogFixture {
    cases: Vec<LogCase>,
}

#[derive(Debug, Deserialize)]
struct LogCase {
    name: String,
    content: String,
    snapshot: ExpectedSnapshot,
}

#[derive(Debug, Deserialize)]
struct ExpectedSnapshot {
    available: bool,
    started_new_log: bool,
    path: String,
    lines: Vec<String>,
    state: String,
    place_id: u64,
    user_id: u64,
    universe_id: u64,
    job_id: String,
    client_channel: String,
    client_roblox_git_hash: String,
    server_roblox_git_hash: String,
    server_prefix: String,
    server_luau_version: String,
    resolution_width: u16,
    resolution_height: u16,
    server: ExpectedServer,
}

#[derive(Debug, Deserialize)]
struct ExpectedServer {
    #[serde(rename = "type")]
    server_type: String,
    rcc_address: String,
    rcc_port: u16,
    udmux_address: String,
    udmux_port: u16,
}

fn temp_log_directory(case_index: usize) -> std::path::PathBuf {
    let unique =
        SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |duration| duration.as_nanos());
    let directory =
        std::env::temp_dir().join(format!("smu-roblox-cpp-fixture-{unique}-{case_index}"));
    fs::create_dir_all(&directory).expect("temporary log directory should be creatable");
    directory
}

fn assert_snapshot_matches(actual: &RobloxLogSnapshot, expected: &ExpectedSnapshot, case: &str) {
    assert_eq!(actual.available, expected.available, "case {case} availability");
    assert_eq!(actual.started_new_log, expected.started_new_log, "case {case} new-log flag");
    assert_eq!(
        actual.path.file_name().and_then(|name| name.to_str()),
        Some(expected.path.as_str()),
        "case {case} path"
    );
    assert_eq!(actual.lines, expected.lines, "case {case} lines");
    assert_eq!(actual.state, expected.state, "case {case} state");
    assert_eq!(actual.place_id, expected.place_id, "case {case} place id");
    assert_eq!(actual.user_id, expected.user_id, "case {case} user id");
    assert_eq!(actual.universe_id, expected.universe_id, "case {case} universe id");
    assert_eq!(actual.job_id, expected.job_id, "case {case} job id");
    assert_eq!(actual.client_channel, expected.client_channel, "case {case} client channel");
    assert_eq!(
        actual.client_roblox_git_hash, expected.client_roblox_git_hash,
        "case {case} client git hash"
    );
    assert_eq!(
        actual.server_roblox_git_hash, expected.server_roblox_git_hash,
        "case {case} server git hash"
    );
    assert_eq!(actual.server_prefix, expected.server_prefix, "case {case} server prefix");
    assert_eq!(
        actual.server_luau_version, expected.server_luau_version,
        "case {case} Luau version"
    );
    assert_eq!(
        (actual.resolution_width, actual.resolution_height),
        (expected.resolution_width, expected.resolution_height),
        "case {case} resolution"
    );
    assert_eq!(actual.server.r#type, expected.server.server_type, "case {case} server type");
    assert_eq!(
        (actual.server.rcc_address.as_str(), actual.server.rcc_port),
        (expected.server.rcc_address.as_str(), expected.server.rcc_port),
        "case {case} RCC server"
    );
    assert_eq!(
        (actual.server.udmux_address.as_str(), actual.server.udmux_port),
        (expected.server.udmux_address.as_str(), expected.server.udmux_port),
        "case {case} UDMUX server"
    );
}

#[test]
fn cpp_generated_log_fixtures_match_rust_reader() {
    let fixture: LogFixture =
        serde_json::from_str(LOG_FIXTURE).expect("C++ generated Roblox log fixture should parse");

    for (case_index, case) in fixture.cases.into_iter().enumerate() {
        let directory = temp_log_directory(case_index);
        let log_path = directory.join("Player_test.log");
        fs::write(&log_path, &case.content).expect("fixture log should be writable");

        let mut reader = RobloxLogReader::new(vec![directory.clone()]);
        let snapshot = reader.poll(true);
        assert_snapshot_matches(&snapshot, &case.snapshot, &case.name);

        fs::remove_dir_all(directory).expect("temporary log directory should be removable");
    }
}
