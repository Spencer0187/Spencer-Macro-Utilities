//! Network lag-switch policy and helper-command construction.
//!
//! Privileged packet interception is a transport concern. This crate owns the deterministic config
//! arbitration and command grammar so Windows/Linux transports can share one tested policy layer.

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "windows")]
pub mod windows;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LagSwitchTargetMode {
    #[default]
    Roblox,
    All,
    Custom,
}

// This intentionally mirrors the persisted C++ compatibility surface; collapsing the flags into
// enums would change profile/script field semantics during the parity phase.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, PartialEq)]
pub struct LagSwitchConfig {
    pub enabled: bool,
    pub currently_blocking: bool,
    pub inbound_hard_block: bool,
    pub outbound_hard_block: bool,
    pub fake_lag_enabled: bool,
    pub inbound_fake_lag: bool,
    pub outbound_fake_lag: bool,
    pub fake_lag_delay_ms: i32,
    pub target_roblox_only: bool,
    pub use_udp: bool,
    pub use_tcp: bool,
    pub prevent_disconnect: bool,
    pub auto_unblock: bool,
    pub max_duration_seconds: f32,
    pub unblock_duration_ms: i32,
    pub target_mode: LagSwitchTargetMode,
    pub remote_ips: Vec<String>,
    pub remote_ports: Vec<i32>,
    pub include_roblox_dynamic_ips: bool,
}

impl Default for LagSwitchConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            currently_blocking: false,
            inbound_hard_block: true,
            outbound_hard_block: true,
            fake_lag_enabled: false,
            inbound_fake_lag: true,
            outbound_fake_lag: true,
            fake_lag_delay_ms: 0,
            target_roblox_only: true,
            use_udp: true,
            use_tcp: false,
            prevent_disconnect: true,
            auto_unblock: false,
            max_duration_seconds: 9.0,
            unblock_duration_ms: 50,
            target_mode: LagSwitchTargetMode::Roblox,
            remote_ips: Vec::new(),
            remote_ports: Vec::new(),
            include_roblox_dynamic_ips: false,
        }
    }
}

/// Base/script state arbitration matching the legacy backend's single-owner override semantics.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LagSwitchState {
    base_config: LagSwitchConfig,
    script_config: LagSwitchConfig,
    script_config_owner: usize,
    script_blocking_owner: usize,
    has_script_config: bool,
    base_blocking: bool,
    script_blocking: bool,
}

impl LagSwitchState {
    #[must_use]
    pub const fn base_config(&self) -> &LagSwitchConfig {
        &self.base_config
    }

    pub fn set_base_config(&mut self, config: LagSwitchConfig) {
        self.base_config = config;
    }

    #[must_use]
    pub const fn base_blocking(&self) -> bool {
        self.base_blocking
    }

    pub const fn set_base_blocking(&mut self, active: bool) {
        self.base_blocking = active;
    }

    pub const fn set_script_blocking(&mut self, owner_token: usize, active: bool) {
        self.script_blocking_owner = owner_token;
        self.script_blocking = active;
    }

    pub fn set_script_config_override(&mut self, owner_token: usize, config: LagSwitchConfig) {
        self.script_config_owner = owner_token;
        self.script_config = config;
        self.has_script_config = true;
    }

    pub fn clear_script_config_override(&mut self, owner_token: usize) {
        if self.has_script_config && self.script_config_owner == owner_token {
            self.has_script_config = false;
            self.script_config = LagSwitchConfig::default();
        }
    }

    pub fn clear_script_state(&mut self, owner_token: usize) {
        self.clear_script_config_override(owner_token);
        if self.script_blocking_owner == owner_token {
            self.script_blocking = false;
        }
    }

    #[must_use]
    pub fn effective_config(&self) -> LagSwitchConfig {
        let mut config = if self.has_script_config {
            self.script_config.clone()
        } else {
            self.base_config.clone()
        };
        config.currently_blocking = self.base_blocking || self.script_blocking;
        config.enabled |= config.currently_blocking;
        config
    }
}

/// Runtime lag-switch capability consumed by the app and scripting layers.
pub trait LagSwitchControl: Send + Sync {
    fn initialize(&self) -> bool;
    fn is_available(&self) -> bool;
    fn effective_config(&self) -> LagSwitchConfig;
    fn is_blocking_active(&self) -> bool;
    fn is_base_blocking_active(&self) -> bool;
    fn set_base_blocking_active(&self, active: bool);
    fn set_base_config(&self, config: LagSwitchConfig);
    fn unsupported_reason(&self) -> String;
    fn set_script_blocking_active(&self, owner_token: usize, active: bool);
    fn set_script_config_override(&self, owner_token: usize, config: LagSwitchConfig);
    fn clear_script_config_override(&self, owner_token: usize);

    fn clear_script_state(&self, owner_token: usize) {
        self.set_script_blocking_active(owner_token, false);
        self.clear_script_config_override(owner_token);
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LagCommandError(&'static str);

impl std::fmt::Display for LagCommandError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for LagCommandError {}

#[must_use]
pub fn is_valid_ipv4_address(value: &str) -> bool {
    if value.is_empty() || value.len() > 15 || value.as_bytes().contains(&0) {
        return false;
    }
    let mut parts = 0_u8;
    let mut index = 0_usize;
    let bytes = value.as_bytes();
    while index < bytes.len() {
        if parts >= 4 || !bytes[index].is_ascii_digit() {
            return false;
        }
        let mut octet = 0_u16;
        let mut digits = 0_u8;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            octet = octet.saturating_mul(10).saturating_add(u16::from(bytes[index] - b'0'));
            digits = digits.saturating_add(1);
            if octet > 255 || digits > 3 {
                return false;
            }
            index += 1;
        }
        parts = parts.saturating_add(1);
        if parts == 4 {
            return index == bytes.len();
        }
        if index >= bytes.len() || bytes[index] != b'.' {
            return false;
        }
        index += 1;
    }
    false
}

fn join_strings(values: &[String]) -> String {
    if values.is_empty() { "-".to_owned() } else { values.join(",") }
}

fn join_ports(values: &[i32]) -> String {
    if values.is_empty() {
        return "-".to_owned();
    }
    values.iter().map(i32::to_string).collect::<Vec<_>>().join(",")
}

const WINDOWS_ROBLOX_RANGE_FILTER: &str = "((ip.SrcAddr >= 128.116.0.0 and ip.SrcAddr <= 128.116.255.255) or (ip.DstAddr >= 128.116.0.0 and ip.DstAddr <= 128.116.255.255))";

const fn windows_protocol_filter(config: &LagSwitchConfig) -> &'static str {
    match (config.use_udp, config.use_tcp) {
        (true, true) => "(udp or tcp)",
        (true, false) => "udp",
        (false, true) => "tcp",
        (false, false) => "false",
    }
}

fn windows_process_udp_port_filter(process_udp_ports: &[i32]) -> String {
    let mut ports = process_udp_ports.to_vec();
    ports.sort_unstable();
    ports.dedup();
    if ports.is_empty() {
        return String::new();
    }
    let clauses = ports
        .into_iter()
        .map(|port| {
            format!("((outbound and udp.SrcPort == {port}) or (inbound and udp.DstPort == {port}))")
        })
        .collect::<Vec<_>>();
    format!("({})", clauses.join(" or "))
}

fn windows_roblox_ip_filter(roblox_dynamic_ips: &[String]) -> String {
    let mut ips = roblox_dynamic_ips.to_vec();
    ips.sort_unstable();
    ips.dedup();
    let mut filter = WINDOWS_ROBLOX_RANGE_FILTER.to_owned();
    for ip in ips {
        filter.push_str(" or (ip.SrcAddr == ");
        filter.push_str(&ip);
        filter.push_str(" or ip.DstAddr == ");
        filter.push_str(&ip);
        filter.push(')');
    }
    format!("({filter})")
}

fn windows_roblox_filter(
    roblox_dynamic_ips: &[String],
    process_udp_ports: &[i32],
    target_process_present: bool,
) -> String {
    let process_filter = windows_process_udp_port_filter(process_udp_ports);
    if !process_filter.is_empty() {
        return process_filter;
    }
    if !target_process_present {
        return "false".to_owned();
    }
    windows_roblox_ip_filter(roblox_dynamic_ips)
}

fn windows_custom_target_filter(config: &LagSwitchConfig, roblox_dynamic_ips: &[String]) -> String {
    let mut clauses = Vec::new();
    if config.include_roblox_dynamic_ips {
        clauses.push(windows_roblox_ip_filter(roblox_dynamic_ips));
    }
    for ip in &config.remote_ips {
        clauses.push(format!(
            "((outbound and ip.DstAddr == {ip}) or (inbound and ip.SrcAddr == {ip}))"
        ));
    }
    for port in &config.remote_ports {
        let mut port_clauses = Vec::new();
        if config.use_udp {
            port_clauses.push(format!(
                "((outbound and udp.DstPort == {port}) or (inbound and udp.SrcPort == {port}))"
            ));
        }
        if config.use_tcp {
            port_clauses.push(format!(
                "((outbound and tcp.DstPort == {port}) or (inbound and tcp.SrcPort == {port}))"
            ));
        }
        match port_clauses.len() {
            1 => clauses.push(port_clauses.remove(0)),
            2 => clauses.push(format!("({} or {})", port_clauses[0], port_clauses[1])),
            _ => {}
        }
    }
    if clauses.is_empty() {
        return "false".to_owned();
    }
    format!("({})", clauses.join(" or "))
}

/// Build the Windows `WinDivert` filter using the same grammar and targeting precedence as C++.
#[must_use]
pub fn build_windows_windivert_filter(
    config: &LagSwitchConfig,
    roblox_dynamic_ips: &[String],
    process_udp_ports: &[i32],
    target_process_present: bool,
) -> String {
    let capture_inbound =
        config.inbound_hard_block || (config.fake_lag_enabled && config.inbound_fake_lag);
    let capture_outbound =
        config.outbound_hard_block || (config.fake_lag_enabled && config.outbound_fake_lag);
    let direction = match (capture_inbound, capture_outbound) {
        (true, true) => "(inbound or outbound)",
        (true, false) => "inbound",
        (false, true) => "outbound",
        (false, false) => "false",
    };
    let target = match config.target_mode {
        LagSwitchTargetMode::All => "true".to_owned(),
        LagSwitchTargetMode::Custom => windows_custom_target_filter(config, roblox_dynamic_ips),
        LagSwitchTargetMode::Roblox => {
            windows_roblox_filter(roblox_dynamic_ips, process_udp_ports, target_process_present)
        }
    };
    format!("{direction} and {target} and {}", windows_protocol_filter(config))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowsPacketAction {
    Drop,
    Send,
    DelayMs(u64),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindowsPacketDecision {
    pub action: WindowsPacketAction,
    pub safety_pulse_active: bool,
    pub reset_safety_clock: bool,
}

#[must_use]
pub const fn windows_safety_pulse_interval_ms(config: &LagSwitchConfig, is_rcc: bool) -> u64 {
    if is_rcc {
        9_500
    } else if config.inbound_hard_block && config.outbound_hard_block {
        19_900
    } else if config.outbound_hard_block {
        290_000
    } else {
        0
    }
}

#[must_use]
pub fn windows_packet_decision(
    config: &LagSwitchConfig,
    outbound: bool,
    is_rcc: bool,
    elapsed_since_safety_pulse_ms: u64,
    safety_pulse_active: bool,
    payload: Option<&[u8]>,
) -> WindowsPacketDecision {
    let send = |active, reset| WindowsPacketDecision {
        action: WindowsPacketAction::Send,
        safety_pulse_active: active,
        reset_safety_clock: reset,
    };
    let drop_packet = |active, reset| WindowsPacketDecision {
        action: WindowsPacketAction::Drop,
        safety_pulse_active: active,
        reset_safety_clock: reset,
    };

    if !config.currently_blocking {
        return send(safety_pulse_active, true);
    }

    let hard_block = if outbound { config.outbound_hard_block } else { config.inbound_hard_block };
    if hard_block {
        if !config.prevent_disconnect {
            return drop_packet(safety_pulse_active, false);
        }

        let interval = windows_safety_pulse_interval_ms(config, is_rcc);
        let mut active =
            safety_pulse_active || (interval > 0 && elapsed_since_safety_pulse_ms >= interval);
        let mut reset_clock = false;
        if interval > 0 && active {
            if elapsed_since_safety_pulse_ms >= interval.saturating_add(250) {
                active = false;
                reset_clock = true;
            } else if payload.is_some() {
                return send(true, false);
            }
        }

        if is_rcc {
            return drop_packet(active, reset_clock);
        }
        if payload.is_some_and(|payload| windows_is_smart_drop_payload(payload, outbound)) {
            return drop_packet(active, reset_clock);
        }
        return send(active, reset_clock);
    }

    let fake_lag_direction =
        if outbound { config.outbound_fake_lag } else { config.inbound_fake_lag };
    if config.fake_lag_enabled && fake_lag_direction && config.fake_lag_delay_ms > 0 {
        let delay_ms = u64::try_from(config.fake_lag_delay_ms).unwrap_or_default();
        return WindowsPacketDecision {
            action: WindowsPacketAction::DelayMs(delay_ms),
            safety_pulse_active,
            reset_safety_clock: false,
        };
    }

    send(safety_pulse_active, true)
}

#[must_use]
pub const fn windows_is_smart_drop_payload(payload: &[u8], outbound: bool) -> bool {
    payload.len() >= 90
        && payload[0] == 0x01
        && payload[1] == 0x00
        && payload[2] == 0x00
        && payload[4] == 0x01
        && payload[5] == 0x11
        && if outbound { payload[3] == 0x1F } else { payload[3] == 0x17 }
}

/// Build the Linux helper's `block ...` command using already-discovered Roblox server IPs.
///
/// # Errors
///
/// Returns the exact compatibility error when directions, protocols, targets, IPs, or ports are
/// invalid for the Linux helper protocol.
pub fn build_linux_target_command(
    config: &LagSwitchConfig,
    roblox_dynamic_ips: &[String],
) -> Result<String, LagCommandError> {
    if !config.inbound_hard_block && !config.outbound_hard_block {
        return Err(LagCommandError(
            "Linux hard blocking requires an inbound or outbound direction.",
        ));
    }
    if !config.use_udp && !config.use_tcp {
        return Err(LagCommandError("Linux hard blocking requires UDP, TCP, or both."));
    }

    let mode = match config.target_mode {
        LagSwitchTargetMode::All => "all",
        LagSwitchTargetMode::Roblox => "roblox",
        LagSwitchTargetMode::Custom => "custom",
    };
    let mut remote_ips = Vec::new();
    let mut remote_ports = Vec::new();

    match config.target_mode {
        LagSwitchTargetMode::Roblox => remote_ips.extend_from_slice(roblox_dynamic_ips),
        LagSwitchTargetMode::Custom => {
            if config.remote_ips.len() > 64 || config.remote_ports.len() > 64 {
                return Err(LagCommandError(
                    "Linux custom targeting supports up to 64 IPs and 64 ports.",
                ));
            }
            for ip in &config.remote_ips {
                if !is_valid_ipv4_address(ip) {
                    return Err(LagCommandError(
                        "Linux custom targeting received an invalid IPv4 address.",
                    ));
                }
                remote_ips.push(ip.clone());
            }
            if config.include_roblox_dynamic_ips {
                remote_ips.extend_from_slice(roblox_dynamic_ips);
            }
            for &port in &config.remote_ports {
                if !(1..=65_535).contains(&port) {
                    return Err(LagCommandError(
                        "Linux custom targeting received an invalid remote port.",
                    ));
                }
                remote_ports.push(port);
            }
            if remote_ips.is_empty() && remote_ports.is_empty() {
                return Err(LagCommandError("Linux custom targeting requires an IP or port."));
            }
        }
        LagSwitchTargetMode::All => {}
    }

    remote_ips.sort_unstable();
    remote_ips.dedup();
    if remote_ips.len() > 64 {
        return Err(LagCommandError("Linux lagswitch supports up to 64 remote IPs."));
    }

    Ok(format!(
        "block in={} out={} udp={} tcp={} mode={mode} ips={} ports={}",
        i32::from(config.inbound_hard_block),
        i32::from(config.outbound_hard_block),
        i32::from(config.use_udp),
        i32::from(config.use_tcp),
        join_strings(&remote_ips),
        join_ports(&remote_ports)
    ))
}

/// Build the command the Linux backend would apply for the current effective config.
///
/// # Errors
///
/// Propagates target/protocol validation errors when an active block or lag command is requested.
pub fn build_linux_apply_command(
    config: &LagSwitchConfig,
    roblox_dynamic_ips: &[String],
) -> Result<String, LagCommandError> {
    if !config.currently_blocking {
        return Ok("reset".to_owned());
    }

    if config.fake_lag_enabled && (config.inbound_fake_lag || config.outbound_fake_lag) {
        let mut lag_config = config.clone();
        lag_config.inbound_hard_block = config.inbound_fake_lag;
        lag_config.outbound_hard_block = config.outbound_fake_lag;
        let block = build_linux_target_command(&lag_config, roblox_dynamic_ips)?;
        let delay = config.fake_lag_delay_ms.clamp(1, 5_000);
        return Ok(format!("lag delay={delay}{}", block.strip_prefix("block").unwrap_or_default()));
    }

    if config.inbound_hard_block || config.outbound_hard_block {
        return build_linux_target_command(config, roblox_dynamic_ips);
    }
    Ok("reset".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipv4_validation_matches_cpp_manual_parser() {
        for valid in ["0.0.0.0", "255.255.255.255", "001.002.003.004", "1.2.3.4"] {
            assert!(is_valid_ipv4_address(valid), "{valid}");
        }
        for invalid in ["", "1.2.3", "1.2.3.4.5", "256.1.1.1", "1..2.3", "+1.2.3.4", "1.2.3.0044"] {
            assert!(!is_valid_ipv4_address(invalid), "{invalid}");
        }
    }

    #[test]
    fn windows_windivert_filter_uses_cpp_roblox_target_precedence() {
        let config = LagSwitchConfig::default();
        assert_eq!(
            build_windows_windivert_filter(&config, &[], &[], false),
            "(inbound or outbound) and false and udp"
        );
        assert_eq!(
            build_windows_windivert_filter(
                &config,
                &["9.9.9.9".to_owned()],
                &[6000, 5000, 6000],
                false,
            ),
            "(inbound or outbound) and (((outbound and udp.SrcPort == 5000) or (inbound and udp.DstPort == 5000)) or ((outbound and udp.SrcPort == 6000) or (inbound and udp.DstPort == 6000))) and udp"
        );
    }

    #[test]
    fn windows_windivert_roblox_ip_filter_matches_cpp_set_order() {
        let config = LagSwitchConfig::default();
        let filter = build_windows_windivert_filter(
            &config,
            &["9.9.9.9".to_owned(), "1.2.3.4".to_owned(), "9.9.9.9".to_owned()],
            &[],
            true,
        );
        assert_eq!(
            filter,
            "(inbound or outbound) and (((ip.SrcAddr >= 128.116.0.0 and ip.SrcAddr <= 128.116.255.255) or (ip.DstAddr >= 128.116.0.0 and ip.DstAddr <= 128.116.255.255)) or (ip.SrcAddr == 1.2.3.4 or ip.DstAddr == 1.2.3.4) or (ip.SrcAddr == 9.9.9.9 or ip.DstAddr == 9.9.9.9)) and udp"
        );
    }

    #[test]
    fn windows_windivert_custom_filter_preserves_cpp_clause_order() {
        let config = LagSwitchConfig {
            target_mode: LagSwitchTargetMode::Custom,
            use_udp: true,
            use_tcp: true,
            remote_ips: vec!["2.2.2.2".to_owned(), "1.1.1.1".to_owned()],
            remote_ports: vec![80, 443],
            ..LagSwitchConfig::default()
        };
        assert_eq!(
            build_windows_windivert_filter(&config, &[], &[], false),
            "(inbound or outbound) and (((outbound and ip.DstAddr == 2.2.2.2) or (inbound and ip.SrcAddr == 2.2.2.2)) or ((outbound and ip.DstAddr == 1.1.1.1) or (inbound and ip.SrcAddr == 1.1.1.1)) or (((outbound and udp.DstPort == 80) or (inbound and udp.SrcPort == 80)) or ((outbound and tcp.DstPort == 80) or (inbound and tcp.SrcPort == 80))) or (((outbound and udp.DstPort == 443) or (inbound and udp.SrcPort == 443)) or ((outbound and tcp.DstPort == 443) or (inbound and tcp.SrcPort == 443)))) and (udp or tcp)"
        );
    }

    #[test]
    fn windows_windivert_fake_lag_direction_matches_cpp_capture_rule() {
        let config = LagSwitchConfig {
            inbound_hard_block: false,
            outbound_hard_block: false,
            fake_lag_enabled: true,
            inbound_fake_lag: false,
            outbound_fake_lag: true,
            target_mode: LagSwitchTargetMode::All,
            use_udp: false,
            use_tcp: true,
            ..LagSwitchConfig::default()
        };
        assert_eq!(
            build_windows_windivert_filter(&config, &[], &[], false),
            "outbound and true and tcp"
        );
    }

    #[test]
    fn windows_windivert_custom_port_without_protocol_matches_cpp_false_target() {
        let config = LagSwitchConfig {
            target_mode: LagSwitchTargetMode::Custom,
            use_udp: false,
            use_tcp: false,
            remote_ports: vec![443],
            ..LagSwitchConfig::default()
        };
        assert_eq!(
            build_windows_windivert_filter(&config, &[], &[], false),
            "(inbound or outbound) and false and false"
        );
    }

    #[test]
    fn windows_safety_pulse_intervals_match_cpp_modes() {
        let both = LagSwitchConfig::default();
        assert_eq!(windows_safety_pulse_interval_ms(&both, false), 19_900);
        assert_eq!(windows_safety_pulse_interval_ms(&both, true), 9_500);

        let outbound = LagSwitchConfig {
            inbound_hard_block: false,
            outbound_hard_block: true,
            ..LagSwitchConfig::default()
        };
        assert_eq!(windows_safety_pulse_interval_ms(&outbound, false), 290_000);

        let inbound = LagSwitchConfig {
            inbound_hard_block: true,
            outbound_hard_block: false,
            ..LagSwitchConfig::default()
        };
        assert_eq!(windows_safety_pulse_interval_ms(&inbound, false), 0);
    }

    #[test]
    fn windows_smart_drop_signature_matches_cpp_inbound_and_outbound_bytes() {
        let mut payload = vec![0_u8; 90];
        payload[0] = 0x01;
        payload[1] = 0x00;
        payload[2] = 0x00;
        payload[4] = 0x01;
        payload[5] = 0x11;

        payload[3] = 0x17;
        assert!(windows_is_smart_drop_payload(&payload, false));
        assert!(!windows_is_smart_drop_payload(&payload, true));

        payload[3] = 0x1F;
        assert!(windows_is_smart_drop_payload(&payload, true));
        assert!(!windows_is_smart_drop_payload(&payload, false));
        assert!(!windows_is_smart_drop_payload(&payload[..89], false));
    }

    #[test]
    fn windows_packet_decision_preserves_cpp_hard_block_and_rcc_pulse_rules() {
        let config = LagSwitchConfig { currently_blocking: true, ..LagSwitchConfig::default() };
        let payload = vec![0xAA; 90];

        assert_eq!(
            windows_packet_decision(&config, true, false, 19_899, false, Some(&payload)),
            WindowsPacketDecision {
                action: WindowsPacketAction::Send,
                safety_pulse_active: false,
                reset_safety_clock: false,
            }
        );
        assert_eq!(
            windows_packet_decision(&config, true, false, 19_900, false, Some(&payload)),
            WindowsPacketDecision {
                action: WindowsPacketAction::Send,
                safety_pulse_active: true,
                reset_safety_clock: false,
            }
        );
        assert_eq!(
            windows_packet_decision(&config, true, false, 20_150, true, Some(&payload)),
            WindowsPacketDecision {
                action: WindowsPacketAction::Send,
                safety_pulse_active: false,
                reset_safety_clock: true,
            }
        );

        assert_eq!(
            windows_packet_decision(&config, true, true, 9_499, false, Some(&payload)).action,
            WindowsPacketAction::Drop
        );
        assert_eq!(
            windows_packet_decision(&config, true, true, 9_500, false, Some(&payload)).action,
            WindowsPacketAction::Send
        );
        assert_eq!(
            windows_packet_decision(&config, true, true, 9_750, true, Some(&payload)),
            WindowsPacketDecision {
                action: WindowsPacketAction::Drop,
                safety_pulse_active: false,
                reset_safety_clock: true,
            }
        );
    }

    #[test]
    fn windows_packet_decision_preserves_cpp_fake_lag_and_passthrough_clock_rules() {
        let fake_lag = LagSwitchConfig {
            currently_blocking: true,
            inbound_hard_block: false,
            outbound_hard_block: false,
            fake_lag_enabled: true,
            inbound_fake_lag: false,
            outbound_fake_lag: true,
            fake_lag_delay_ms: 321,
            ..LagSwitchConfig::default()
        };
        assert_eq!(
            windows_packet_decision(&fake_lag, true, false, 123, true, None),
            WindowsPacketDecision {
                action: WindowsPacketAction::DelayMs(321),
                safety_pulse_active: true,
                reset_safety_clock: false,
            }
        );
        assert_eq!(
            windows_packet_decision(&fake_lag, false, false, 123, true, None),
            WindowsPacketDecision {
                action: WindowsPacketAction::Send,
                safety_pulse_active: true,
                reset_safety_clock: true,
            }
        );

        let inactive = LagSwitchConfig { currently_blocking: false, ..fake_lag };
        assert_eq!(
            windows_packet_decision(&inactive, true, false, 123, true, None),
            WindowsPacketDecision {
                action: WindowsPacketAction::Send,
                safety_pulse_active: true,
                reset_safety_clock: true,
            }
        );
    }

    #[test]
    fn defaults_match_cpp_network_backend_contract() {
        assert_eq!(
            LagSwitchConfig::default(),
            LagSwitchConfig {
                enabled: false,
                currently_blocking: false,
                inbound_hard_block: true,
                outbound_hard_block: true,
                fake_lag_enabled: false,
                inbound_fake_lag: true,
                outbound_fake_lag: true,
                fake_lag_delay_ms: 0,
                target_roblox_only: true,
                use_udp: true,
                use_tcp: false,
                prevent_disconnect: true,
                auto_unblock: false,
                max_duration_seconds: 9.0,
                unblock_duration_ms: 50,
                target_mode: LagSwitchTargetMode::Roblox,
                remote_ips: Vec::new(),
                remote_ports: Vec::new(),
                include_roblox_dynamic_ips: false,
            }
        );
    }

    #[test]
    fn script_config_and_blocking_owners_match_cpp_single_owner_semantics() {
        let mut state = LagSwitchState::default();
        state.set_base_config(LagSwitchConfig {
            fake_lag_delay_ms: 10,
            ..LagSwitchConfig::default()
        });
        state.set_base_blocking(true);
        state.set_script_config_override(
            100,
            LagSwitchConfig { fake_lag_delay_ms: 20, ..LagSwitchConfig::default() },
        );
        state.set_script_blocking(200, true);

        let effective = state.effective_config();
        assert_eq!(effective.fake_lag_delay_ms, 20);
        assert!(effective.currently_blocking);
        assert!(effective.enabled);

        state.clear_script_state(100);
        assert_eq!(state.effective_config().fake_lag_delay_ms, 10);
        assert!(state.effective_config().currently_blocking);
        state.clear_script_state(999);
        assert!(state.effective_config().currently_blocking);
        state.clear_script_state(200);
        assert!(state.effective_config().currently_blocking, "base block remains active");
        state.set_base_blocking(false);
        assert!(!state.effective_config().currently_blocking);
    }

    #[test]
    fn setting_new_script_owner_replaces_previous_owner() {
        let mut state = LagSwitchState::default();
        state.set_script_config_override(
            1,
            LagSwitchConfig { fake_lag_delay_ms: 111, ..LagSwitchConfig::default() },
        );
        state.set_script_config_override(
            2,
            LagSwitchConfig { fake_lag_delay_ms: 222, ..LagSwitchConfig::default() },
        );
        state.clear_script_config_override(1);
        assert_eq!(state.effective_config().fake_lag_delay_ms, 222);
        state.clear_script_config_override(2);
        assert_eq!(state.effective_config().fake_lag_delay_ms, 0);
    }

    #[test]
    fn linux_roblox_command_sorts_and_deduplicates_dynamic_ips() {
        let config = LagSwitchConfig {
            currently_blocking: true,
            target_mode: LagSwitchTargetMode::Roblox,
            use_tcp: true,
            ..LagSwitchConfig::default()
        };
        assert_eq!(
            build_linux_apply_command(
                &config,
                &["128.116.2.3".to_owned(), "10.1.2.3".to_owned(), "128.116.2.3".to_owned()]
            ),
            Ok("block in=1 out=1 udp=1 tcp=1 mode=roblox ips=10.1.2.3,128.116.2.3 ports=-"
                .to_owned())
        );
    }

    #[test]
    fn linux_custom_target_preserves_port_order_but_sorts_ips() {
        let config = LagSwitchConfig {
            currently_blocking: true,
            target_mode: LagSwitchTargetMode::Custom,
            remote_ips: vec!["8.8.8.8".to_owned(), "1.1.1.1".to_owned(), "8.8.8.8".to_owned()],
            remote_ports: vec![443, 80, 443],
            ..LagSwitchConfig::default()
        };
        assert_eq!(
            build_linux_apply_command(&config, &[]),
            Ok("block in=1 out=1 udp=1 tcp=0 mode=custom ips=1.1.1.1,8.8.8.8 ports=443,80,443"
                .to_owned())
        );
    }

    #[test]
    fn linux_fake_lag_reuses_target_policy_and_clamps_delay() {
        let config = LagSwitchConfig {
            currently_blocking: true,
            fake_lag_enabled: true,
            inbound_fake_lag: false,
            outbound_fake_lag: true,
            fake_lag_delay_ms: 9_999,
            target_mode: LagSwitchTargetMode::All,
            ..LagSwitchConfig::default()
        };
        assert_eq!(
            build_linux_apply_command(&config, &[]),
            Ok("lag delay=5000 in=0 out=1 udp=1 tcp=0 mode=all ips=- ports=-".to_owned())
        );

        let minimum = LagSwitchConfig { fake_lag_delay_ms: -50, ..config };
        assert_eq!(
            build_linux_apply_command(&minimum, &[]),
            Ok("lag delay=1 in=0 out=1 udp=1 tcp=0 mode=all ips=- ports=-".to_owned())
        );
    }

    #[test]
    fn linux_reset_short_circuits_invalid_inactive_config() {
        let config = LagSwitchConfig {
            currently_blocking: false,
            inbound_hard_block: false,
            outbound_hard_block: false,
            use_udp: false,
            use_tcp: false,
            ..LagSwitchConfig::default()
        };
        assert_eq!(build_linux_apply_command(&config, &[]), Ok("reset".to_owned()));
    }

    #[test]
    fn linux_validation_errors_match_cpp_messages() {
        let no_direction = LagSwitchConfig {
            currently_blocking: true,
            inbound_hard_block: false,
            outbound_hard_block: false,
            target_mode: LagSwitchTargetMode::All,
            ..LagSwitchConfig::default()
        };
        assert_eq!(
            build_linux_target_command(&no_direction, &[]).map_err(|error| error.to_string()),
            Err("Linux hard blocking requires an inbound or outbound direction.".to_owned())
        );

        let no_protocol =
            LagSwitchConfig { use_udp: false, use_tcp: false, ..LagSwitchConfig::default() };
        assert_eq!(
            build_linux_target_command(&no_protocol, &[]).map_err(|error| error.to_string()),
            Err("Linux hard blocking requires UDP, TCP, or both.".to_owned())
        );

        let invalid_ip = LagSwitchConfig {
            target_mode: LagSwitchTargetMode::Custom,
            remote_ips: vec!["999.1.2.3".to_owned()],
            ..LagSwitchConfig::default()
        };
        assert_eq!(
            build_linux_target_command(&invalid_ip, &[]).map_err(|error| error.to_string()),
            Err("Linux custom targeting received an invalid IPv4 address.".to_owned())
        );

        let invalid_port = LagSwitchConfig {
            target_mode: LagSwitchTargetMode::Custom,
            remote_ports: vec![65_536],
            ..LagSwitchConfig::default()
        };
        assert_eq!(
            build_linux_target_command(&invalid_port, &[]).map_err(|error| error.to_string()),
            Err("Linux custom targeting received an invalid remote port.".to_owned())
        );

        let empty_custom = LagSwitchConfig {
            target_mode: LagSwitchTargetMode::Custom,
            ..LagSwitchConfig::default()
        };
        assert_eq!(
            build_linux_target_command(&empty_custom, &[]).map_err(|error| error.to_string()),
            Err("Linux custom targeting requires an IP or port.".to_owned())
        );
    }

    #[test]
    fn custom_dynamic_ips_can_push_post_merge_limit_over_64() {
        let config = LagSwitchConfig {
            target_mode: LagSwitchTargetMode::Custom,
            remote_ips: vec!["1.1.1.1".to_owned()],
            include_roblox_dynamic_ips: true,
            ..LagSwitchConfig::default()
        };
        let dynamic = (1..=64).map(|last| format!("10.0.0.{last}")).collect::<Vec<_>>();
        assert_eq!(
            build_linux_target_command(&config, &dynamic).map_err(|error| error.to_string()),
            Err("Linux lagswitch supports up to 64 remote IPs.".to_owned())
        );
    }
}
