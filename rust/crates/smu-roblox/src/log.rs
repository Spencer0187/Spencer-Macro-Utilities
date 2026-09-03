//! Incremental Roblox player-log discovery and compatibility parsing.

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::time::SystemTime;

const CPP_LINE_BUFFER_PAYLOAD: usize = 4095;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RobloxLogServerInfo {
    pub r#type: String,
    pub rcc_address: String,
    pub rcc_port: u16,
    pub udmux_address: String,
    pub udmux_port: u16,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RobloxLogSnapshot {
    pub available: bool,
    pub started_new_log: bool,
    pub path: PathBuf,
    pub lines: Vec<String>,
    pub state: String,
    pub place_id: u64,
    pub user_id: u64,
    pub universe_id: u64,
    pub job_id: String,
    pub client_channel: String,
    pub client_roblox_git_hash: String,
    pub server_roblox_git_hash: String,
    pub server_prefix: String,
    pub server_luau_version: String,
    pub resolution_width: u16,
    pub resolution_height: u16,
    pub server: RobloxLogServerInfo,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum ParserState {
    LuaApp,
    InGame,
    Offline,
    #[default]
    Unknown,
}

impl ParserState {
    const fn as_str(self) -> &'static str {
        match self {
            Self::LuaApp => "lua_app",
            Self::InGame => "in_game",
            Self::Offline => "offline",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum ServerType {
    Rcc,
    Udmux,
    #[default]
    Unknown,
}

impl ServerType {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Rcc => "rcc",
            Self::Udmux => "udmux",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct ParserServerInfo {
    server_type: ServerType,
    rcc_address: String,
    rcc_port: u16,
    udmux_address: String,
    udmux_port: u16,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct LogParserState {
    last_idx: u32,
    state: ParserState,
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
    server: ParserServerInfo,
}

impl LogParserState {
    fn parse_chunk(&mut self, line: &str) {
        if line.contains("returnToLuaApp") {
            self.state = ParserState::LuaApp;
        }
        if line.contains("setStage: (stage:None)") {
            self.state = ParserState::Offline;
        }
        if line.contains("Report game_join_loadtime") {
            self.state = ParserState::InGame;
            if let Some(value) = find_comma_value(line, "placeid:").filter(|value| *value != 0) {
                self.place_id = value;
            }
            if let Some(value) = find_comma_value(line, "userid:").filter(|value| *value != 0) {
                self.user_id = value;
            }
            if let Some(value) = find_comma_value(line, "universeid:").filter(|value| *value != 0) {
                self.universe_id = value;
            }
        }
        if line.contains("Joining game '") {
            self.parse_joining_game(line);
        }
        if line.contains("Connecting to") {
            self.parse_connecting_to(line);
        }
        if line.contains("UDMUX Address") {
            self.parse_udmux_address(line);
        }
        if line.contains("Server RobloxGitHash") {
            extract_after_colon(line, "Server RobloxGitHash")
                .unwrap_or_default()
                .clone_into(&mut self.server_roblox_git_hash);
        }
        if line.contains("Server Prefix") {
            extract_after_colon(line, "Server Prefix")
                .unwrap_or_default()
                .clone_into(&mut self.server_prefix);
            let luau_version = extract_luau_version(&self.server_prefix).unwrap_or_default();
            luau_version.clone_into(&mut self.server_luau_version);
        }
        if line.contains("[FLog::ClientRunInfo] RobloxGitHash") {
            extract_after_colon(line, "RobloxGitHash")
                .unwrap_or_default()
                .clone_into(&mut self.client_roblox_git_hash);
        }
        if line.contains("[FLog::ClientRunInfo] The channel is") {
            self.parse_channel(line);
        }
        if line.contains("resizing main targets to") {
            self.parse_resolution(line);
        }
    }

    fn parse_joining_game(&mut self, line: &str) {
        let Some((_, tail)) = line.split_once("Joining game '") else {
            return;
        };
        let Some((job_id, _)) = tail.split_once('\'') else {
            return;
        };
        job_id.clone_into(&mut self.job_id);
    }

    fn server_needs_address(&self) -> bool {
        self.server.server_type == ServerType::Unknown
            || self.server.rcc_address.is_empty()
            || self.server.rcc_port == 0
    }

    fn parse_connecting_to(&mut self, line: &str) {
        if !self.server_needs_address() {
            return;
        }
        if line.contains("UDMUX") && line.contains("RCC") {
            self.server.server_type = ServerType::Udmux;
            let Some((_, udmux_tail)) = line.split_once("UDMUX server ") else {
                return;
            };
            let Some((udmux_address, udmux_port)) = extract_ip(udmux_tail) else {
                return;
            };
            if udmux_address.len() >= 64 {
                return;
            }
            udmux_address.clone_into(&mut self.server.udmux_address);
            self.server.udmux_port = udmux_port;

            let Some((_, rcc_tail)) = line.split_once("RCC server ") else {
                return;
            };
            let Some((rcc_address, rcc_port)) = extract_ip(rcc_tail) else {
                return;
            };
            if rcc_address.len() >= 64 {
                return;
            }
            rcc_address.clone_into(&mut self.server.rcc_address);
            self.server.rcc_port = rcc_port;
            return;
        }

        self.server.server_type = ServerType::Rcc;
        let Some((_, tail)) = line.split_once("Connecting to ") else {
            return;
        };
        let Some((address, port)) = extract_ip(tail) else {
            return;
        };
        if address.len() >= 64 {
            return;
        }
        address.clone_into(&mut self.server.rcc_address);
        self.server.rcc_port = port;
    }

    fn parse_udmux_address(&mut self, line: &str) {
        if !self.server_needs_address() {
            return;
        }
        let Some((_, udmux_tail)) = line.split_once("UDMUX Address = ") else {
            return;
        };
        let Some((udmux_address, after_udmux)) = udmux_tail.split_once(',') else {
            return;
        };
        if udmux_address.len() >= 64 {
            return;
        }
        udmux_address.clone_into(&mut self.server.udmux_address);

        let Some((_, udmux_port_tail)) = after_udmux.split_once("Port = ") else {
            return;
        };
        self.server.udmux_port = parse_c_u16_prefix(udmux_port_tail);

        let Some((_, rcc_tail)) = line.split_once("RCC Server Address = ") else {
            return;
        };
        let Some((rcc_address, after_rcc)) = rcc_tail.split_once(',') else {
            return;
        };
        if rcc_address.len() >= 64 {
            return;
        }
        rcc_address.clone_into(&mut self.server.rcc_address);

        let Some((_, rcc_port_tail)) = after_rcc.split_once("Port = ") else {
            return;
        };
        self.server.rcc_port = parse_c_u16_prefix(rcc_port_tail);
        self.server.server_type = ServerType::Udmux;
    }

    fn parse_channel(&mut self, line: &str) {
        let Some((_, tail)) = line.split_once("The channel is ") else {
            return;
        };
        trim_crlf(tail).clone_into(&mut self.client_channel);
    }

    fn parse_resolution(&mut self, line: &str) {
        let Some((_, tail)) = line.split_once("resizing main targets to ") else {
            return;
        };
        let Some((width, height)) = tail.split_once('x') else {
            return;
        };
        let height = trim_crlf(height);
        if width.len() >= 16 || height.len() >= 16 {
            return;
        }
        self.resolution_width = parse_c_u16_prefix(width);
        self.resolution_height = parse_c_u16_prefix(height);
    }

    fn populate_snapshot(&self, snapshot: &mut RobloxLogSnapshot) {
        self.state.as_str().clone_into(&mut snapshot.state);
        snapshot.place_id = self.place_id;
        snapshot.user_id = self.user_id;
        snapshot.universe_id = self.universe_id;
        snapshot.job_id.clone_from(&self.job_id);
        snapshot.client_channel.clone_from(&self.client_channel);
        snapshot.client_roblox_git_hash.clone_from(&self.client_roblox_git_hash);
        snapshot.server_roblox_git_hash.clone_from(&self.server_roblox_git_hash);
        snapshot.server_prefix.clone_from(&self.server_prefix);
        snapshot.server_luau_version.clone_from(&self.server_luau_version);
        snapshot.resolution_width = self.resolution_width;
        snapshot.resolution_height = self.resolution_height;
        snapshot.server = RobloxLogServerInfo {
            r#type: self.server.server_type.as_str().to_owned(),
            rcc_address: self.server.rcc_address.clone(),
            rcc_port: self.server.rcc_port,
            udmux_address: self.server.udmux_address.clone(),
            udmux_port: self.server.udmux_port,
        };
    }
}

fn trim_crlf(value: &str) -> &str {
    value.trim_end_matches(['\n', '\r'])
}

fn parse_c_u64_prefix(value: &str) -> u64 {
    let bytes = value.as_bytes();
    let mut index = 0;
    while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
        index += 1;
    }

    let negative = match bytes.get(index) {
        Some(b'-') => {
            index += 1;
            true
        }
        Some(b'+') => {
            index += 1;
            false
        }
        _ => false,
    };

    let mut magnitude = 0_u64;
    let mut found_digit = false;
    let mut overflowed = false;
    while let Some(byte) = bytes.get(index).copied().filter(u8::is_ascii_digit) {
        found_digit = true;
        let digit = u64::from(byte - b'0');
        match magnitude.checked_mul(10).and_then(|value| value.checked_add(digit)) {
            Some(value) => magnitude = value,
            None => overflowed = true,
        }
        index += 1;
    }

    if !found_digit {
        return 0;
    }
    if overflowed {
        return u64::MAX;
    }
    if negative { 0_u64.wrapping_sub(magnitude) } else { magnitude }
}

#[allow(clippy::cast_possible_truncation)]
fn parse_c_u16_prefix(value: &str) -> u16 {
    parse_c_u64_prefix(value) as u16
}

fn find_comma_value(line: &str, marker: &str) -> Option<u64> {
    let marker_start = line.find(marker)?;
    let from_marker = line.get(marker_start..)?;
    let colon = from_marker.find(':')?;
    let value = from_marker.get(colon.saturating_add(1)..)?;
    let comma = value.find(',')?;
    Some(parse_c_u64_prefix(value.get(..comma)?))
}

fn extract_ip(value: &str) -> Option<(&str, u16)> {
    let (address, port) = value.split_once(':')?;
    Some((address, parse_c_u16_prefix(port)))
}

fn extract_after_colon<'a>(line: &'a str, label: &str) -> Option<&'a str> {
    let found = line.find(label)?;
    let tail = line.get(found..)?;
    let colon = tail.find(':')?;
    let mut value = tail.get(colon.saturating_add(1)..)?;
    if let Some(stripped) = value.strip_prefix(' ') {
        value = stripped;
    }
    Some(trim_crlf(value))
}

fn extract_luau_version(git_hash: &str) -> Option<&str> {
    let first = git_hash.find('.')?;
    let second_relative = git_hash.get(first.saturating_add(1)..)?.find('.')?;
    git_hash.get(..first.saturating_add(1).saturating_add(second_relative))
}

/// Incrementally reads the newest Roblox player log from configured directories.
#[derive(Debug)]
pub struct RobloxLogReader {
    log_directories: Vec<PathBuf>,
    active_path: PathBuf,
    pending_line: Vec<u8>,
    parser: Option<LogParserState>,
}

impl Default for RobloxLogReader {
    fn default() -> Self {
        Self::new(default_log_directories())
    }
}

impl RobloxLogReader {
    #[must_use]
    pub const fn new(log_directories: Vec<PathBuf>) -> Self {
        Self {
            log_directories,
            active_path: PathBuf::new(),
            pending_line: Vec::new(),
            parser: None,
        }
    }

    fn find_newest_log(&self) -> Option<PathBuf> {
        let mut newest: Option<(SystemTime, PathBuf)> = None;
        for directory in &self.log_directories {
            let Ok(entries) = fs::read_dir(directory) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|extension| extension.to_str()) != Some("log") {
                    continue;
                }
                let Ok(metadata) = entry.metadata() else {
                    continue;
                };
                if !metadata.is_file() {
                    continue;
                }
                let Ok(modified) = metadata.modified() else {
                    continue;
                };
                if newest.as_ref().is_none_or(|(time, _)| modified > *time) {
                    newest = Some((modified, path));
                }
            }
        }
        newest.map(|(_, path)| path)
    }

    fn reset(&mut self, path: PathBuf) {
        self.active_path = path;
        self.pending_line.clear();
        self.parser = Some(LogParserState::default());
    }

    fn clear(&mut self) {
        self.active_path.clear();
        self.pending_line.clear();
        self.parser = None;
    }

    #[must_use]
    pub fn poll(&mut self, include_existing: bool) -> RobloxLogSnapshot {
        let Some(newest) = self.find_newest_log() else {
            self.clear();
            return RobloxLogSnapshot::default();
        };

        let file_size = fs::metadata(&newest).map(|metadata| metadata.len()).ok();
        let file_was_truncated = self.parser.as_ref().is_some_and(|parser| {
            newest == self.active_path
                && file_size.is_some_and(|size| size < u64::from(parser.last_idx))
        });
        let started_new_log =
            self.parser.is_none() || newest != self.active_path || file_was_truncated;
        if started_new_log {
            self.reset(newest);
        }

        let capture_lines = !started_new_log || include_existing;
        let Some((lines, parser_snapshot)) = self.parse_active(capture_lines) else {
            self.clear();
            return RobloxLogSnapshot::default();
        };

        let mut snapshot = RobloxLogSnapshot {
            available: true,
            started_new_log,
            path: self.active_path.clone(),
            lines,
            ..RobloxLogSnapshot::default()
        };
        parser_snapshot.populate_snapshot(&mut snapshot);
        snapshot
    }

    fn parse_active(&mut self, capture_lines: bool) -> Option<(Vec<String>, LogParserState)> {
        let parser = self.parser.as_mut()?;
        let mut file = File::open(&self.active_path).ok()?;
        file.seek(SeekFrom::Start(u64::from(parser.last_idx))).ok()?;
        let mut lines = Vec::new();
        let mut chunk = [0_u8; CPP_LINE_BUFFER_PAYLOAD];

        loop {
            let read = read_cpp_fgets_chunk(&mut file, &mut chunk).ok()?;
            if read == 0 {
                break;
            }
            let bytes = &chunk[..read];
            let text = String::from_utf8_lossy(bytes);
            parser.parse_chunk(&text);
            if capture_lines {
                self.pending_line.extend_from_slice(bytes);
                if self.pending_line.last() == Some(&b'\n') {
                    while matches!(self.pending_line.last(), Some(b'\n' | b'\r')) {
                        self.pending_line.pop();
                    }
                    lines.push(String::from_utf8_lossy(&self.pending_line).into_owned());
                    self.pending_line.clear();
                }
            }
        }

        let position = file.stream_position().ok()?;
        #[allow(clippy::cast_possible_truncation)]
        {
            parser.last_idx = position as u32;
        }
        Some((lines, parser.clone()))
    }
}

fn read_cpp_fgets_chunk(
    file: &mut File,
    buffer: &mut [u8; CPP_LINE_BUFFER_PAYLOAD],
) -> std::io::Result<usize> {
    let mut read = 0;
    while read < buffer.len() {
        let count = file.read(&mut buffer[read..=read])?;
        if count == 0 {
            break;
        }
        read += count;
        if buffer[read - 1] == b'\n' {
            break;
        }
    }
    Ok(read)
}

fn default_log_directories() -> Vec<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        return std::env::var_os("LOCALAPPDATA")
            .filter(|value| !value.is_empty())
            .map(|value| PathBuf::from(value).join("Roblox").join("logs"))
            .into_iter()
            .collect();
    }

    #[cfg(target_os = "linux")]
    {
        let home = std::env::var_os("HOME").filter(|value| !value.is_empty()).map(PathBuf::from);
        let mut directories = Vec::new();
        if let Some(xdg_data) = std::env::var_os("XDG_DATA_HOME").filter(|value| !value.is_empty())
        {
            directories.push(PathBuf::from(xdg_data).join("sober/appData/logs"));
        }
        if let Some(home) = home {
            directories.push(home.join(".local/share/sober/appData/logs"));
            directories.push(home.join(".var/app/org.vinegarhq.Sober/data/sober/appData/logs"));
        }
        return directories;
    }

    #[cfg(target_os = "macos")]
    {
        return std::env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .map(|home| {
                vec![
                    home.join("Library/Logs/Roblox"),
                    home.join("Library/Application Support/Roblox/logs"),
                ]
            })
            .unwrap_or_default();
    }

    #[allow(unreachable_code)]
    Vec::new()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::{LogParserState, RobloxLogReader};
    use std::fs::{self, OpenOptions};
    use std::io::Write;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_log_directory() -> std::path::PathBuf {
        let unique =
            SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |duration| duration.as_nanos());
        let directory = std::env::temp_dir().join(format!("smu-roblox-log-reader-{unique}"));
        fs::create_dir_all(&directory).expect("temporary log directory should be creatable");
        directory
    }

    #[test]
    fn cpp_log_reader_incremental_behavior_matches() {
        let directory = temp_log_directory();
        let log_path = directory.join("Player_test.log");
        fs::write(
            &log_path,
            concat!(
                "Report game_join_loadtime placeid:123, userid:456, universeid:789,\n",
                "Joining game 'test-job'\n",
                "existing console message\n"
            ),
        )
        .expect("initial log fixture should be writable");

        let mut reader = RobloxLogReader::new(vec![directory.clone()]);
        let initial = reader.poll(false);
        assert!(initial.available);
        assert!(initial.started_new_log);
        assert!(initial.lines.is_empty());
        assert_eq!(initial.state, "in_game");
        assert_eq!((initial.place_id, initial.user_id, initial.universe_id), (123, 456, 789));
        assert_eq!(initial.job_id, "test-job");

        let mut log = OpenOptions::new().append(true).open(&log_path).expect("log should open");
        writeln!(log, "new console message").expect("append should succeed");
        drop(log);
        let appended = reader.poll(false);
        assert!(!appended.started_new_log);
        assert_eq!(appended.lines, ["new console message"]);

        let mut log = OpenOptions::new().append(true).open(&log_path).expect("log should open");
        write!(log, "partial console").expect("partial append should succeed");
        drop(log);
        assert!(reader.poll(false).lines.is_empty());

        let mut log = OpenOptions::new().append(true).open(&log_path).expect("log should open");
        writeln!(log, " message").expect("completion append should succeed");
        drop(log);
        assert_eq!(reader.poll(false).lines, ["partial console message"]);

        fs::write(&log_path, "rotated console message\n").expect("log truncation should succeed");
        let truncated = reader.poll(true);
        assert!(truncated.started_new_log);
        assert_eq!(truncated.lines, ["rotated console message"]);
        fs::remove_dir_all(directory).expect("temporary log directory should be removable");
    }

    #[test]
    fn parser_exposes_server_version_channel_and_resolution_metadata() {
        let mut parser = LogParserState::default();
        parser.parse_chunk("Connecting to UDMUX server 10.0.0.1:5000 RCC server 20.0.0.2:6000\n");
        parser.parse_chunk("Server RobloxGitHash: server-hash\n");
        parser.parse_chunk("Server Prefix: 0.650.2.6500123\n");
        parser.parse_chunk("[FLog::ClientRunInfo] RobloxGitHash: client-hash\n");
        parser.parse_chunk("[FLog::ClientRunInfo] The channel is zlive\n");
        parser.parse_chunk("resizing main targets to 1920x1080\n");

        assert_eq!(parser.server.server_type.as_str(), "udmux");
        assert_eq!(
            (parser.server.udmux_address.as_str(), parser.server.udmux_port),
            ("10.0.0.1", 5000)
        );
        assert_eq!(
            (parser.server.rcc_address.as_str(), parser.server.rcc_port),
            ("20.0.0.2", 6000)
        );
        assert_eq!(parser.server_roblox_git_hash, "server-hash");
        assert_eq!(parser.server_prefix, "0.650.2.6500123");
        assert_eq!(parser.server_luau_version, "0.650");
        assert_eq!(parser.client_roblox_git_hash, "client-hash");
        assert_eq!(parser.client_channel, "zlive");
        assert_eq!((parser.resolution_width, parser.resolution_height), (1920, 1080));
    }
}
