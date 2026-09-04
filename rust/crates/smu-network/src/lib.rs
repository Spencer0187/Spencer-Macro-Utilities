//! Network lag-switch policy and helper-command construction.
//!
//! Privileged packet interception is a transport concern. This crate owns the deterministic config
//! arbitration and command grammar so Windows/Linux transports can share one tested policy layer.

#[cfg(target_os = "linux")]
pub mod linux;

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
    fn is_available(&self) -> bool;
    fn effective_config(&self) -> LagSwitchConfig;
    fn is_blocking_active(&self) -> bool;
    fn is_base_blocking_active(&self) -> bool;
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
