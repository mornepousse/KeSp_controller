//! Connection classification and forwarded-command timeout logic.
//!
//! The dongle CDC is transparent: a KS frame is either handled dongle-locally
//! or forwarded to the paired keyboard over ESP-NOW (slower, since it tunnels,
//! chunks and retries over RF). These pure helpers mirror that split so the
//! serial layer can pick read timeouts and the UI can show the connection mode.
//! Unit-tested, no I/O.

/// How the controller is attached to the keyboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionKind {
    /// Keyboard on USB directly.
    Direct,
    /// Through the M.2 dongle, which relays config to the paired keyboard over RF.
    Dongle { paired_count: u8 },
}

/// Commands the dongle handles itself (NOT forwarded to the keyboard) — an exact
/// mirror of the firmware's `cfg_is_dongle_local` list. Everything else is a
/// config command the dongle forwards over ESP-NOW to the paired keyboard.
///
/// Note: the base-protocol handshake commands (PING/FEATURES) are deliberately
/// NOT in this list — they are answered before a connection is even classified,
/// so their absence here only means a forwarded-timeout (harmless) if ever sent
/// post-classification.
pub fn is_dongle_local(cmd_id: u8) -> bool {
    matches!(
        cmd_id,
        0x03      // DFU
            | 0xB1 // NVS_RESET
            | 0xB2 // RF_PAIR_START
            | 0xB3 // RF_STATUS
            | 0xB4 // RF_PAIR_LIST
            | 0xB5 // RF_PAIR_RESET
            | 0xB6 // BATTERY
            | 0xB7 // MONITOR
            | 0xC0 // SEC_SET_SLOT
            | 0xC1 // SEC_CLEAR_SLOT
            | 0xC2 // SEC_LIST
            | 0xC3 // SEC_SELFTEST
            | 0xF0 // OTA_START
            | 0xF1 // OTA_DATA
            | 0xF2 // OTA_ABORT
    )
}

/// Classify from a connect-time probe: `RF_PAIR_LIST` succeeded (a dongle) and
/// yielded `Some(paired_count)`; a failure/unknown response (`None`) means a
/// directly-attached keyboard.
pub fn classify_connection(rf_pair_list_paired: Option<u8>) -> ConnectionKind {
    match rf_pair_list_paired {
        Some(paired_count) => ConnectionKind::Dongle { paired_count },
        None => ConnectionKind::Direct,
    }
}

/// Read timeout for a command. A command FORWARDED through the dongle (the slow
/// ESP-NOW path) gets double the base; a direct connection, or a dongle-local
/// command answered by the dongle itself, keeps the base.
pub fn read_timeout_ms(cmd_id: u8, kind: ConnectionKind, base_ms: u64) -> u64 {
    match kind {
        ConnectionKind::Dongle { .. } if !is_dongle_local(cmd_id) => base_ms * 2,
        _ => base_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dongle_local_commands_are_recognized() {
        for cmd in [
            0x03u8, 0xB1, 0xB2, 0xB3, 0xB4, 0xB5, 0xB6, 0xB7, 0xC0, 0xC1, 0xC2, 0xC3, 0xF0, 0xF1, 0xF2,
        ] {
            assert!(is_dongle_local(cmd), "0x{cmd:02X} should be dongle-local");
        }
    }

    #[test]
    fn config_commands_are_forwarded() {
        // Anything outside the local list is forwarded to the keyboard.
        for cmd in [0x00u8, 0x01, 0x10, 0x20, 0xA0, 0xB0, 0xBF, 0xC4, 0xFF] {
            assert!(!is_dongle_local(cmd), "0x{cmd:02X} should be forwarded");
        }
    }

    #[test]
    fn classify_none_is_direct() {
        assert_eq!(classify_connection(None), ConnectionKind::Direct);
    }

    #[test]
    fn classify_some_is_dongle_with_count() {
        assert_eq!(classify_connection(Some(0)), ConnectionKind::Dongle { paired_count: 0 });
        assert_eq!(classify_connection(Some(2)), ConnectionKind::Dongle { paired_count: 2 });
    }

    #[test]
    fn timeout_direct_is_always_base() {
        assert_eq!(read_timeout_ms(0x10, ConnectionKind::Direct, 1500), 1500);
        assert_eq!(read_timeout_ms(0xB4, ConnectionKind::Direct, 1500), 1500);
    }

    #[test]
    fn timeout_dongle_local_is_base() {
        // A dongle-local command (RF_PAIR_LIST) is answered by the dongle: base.
        assert_eq!(read_timeout_ms(0xB4, ConnectionKind::Dongle { paired_count: 1 }, 1500), 1500);
    }

    #[test]
    fn timeout_dongle_forwarded_is_doubled() {
        // A forwarded config command over the RF tunnel: doubled.
        assert_eq!(read_timeout_ms(0x10, ConnectionKind::Dongle { paired_count: 1 }, 1500), 3000);
        assert_eq!(read_timeout_ms(0x10, ConnectionKind::Dongle { paired_count: 1 }, 3000), 6000);
    }
}
