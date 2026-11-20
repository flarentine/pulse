pub fn bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{n} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

pub fn duration_secs(total_secs: u64) -> String {
    let days = total_secs / 86_400;
    let hours = (total_secs % 86_400) / 3_600;
    let mins = (total_secs % 3_600) / 60;
    let secs = total_secs % 60;

    if days > 0 {
        format!("{days}d {hours:02}h {mins:02}m")
    } else if hours > 0 {
        format!("{hours}h {mins:02}m {secs:02}s")
    } else {
        format!("{mins}m {secs:02}s")
    }
}

pub fn started_at(start_time_secs: u64) -> String {
    if start_time_secs == 0 {
        return "unavailable".to_string();
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(start_time_secs);
    let ago = now.saturating_sub(start_time_secs);
    format!("{} ago", duration_secs(ago))
}

pub fn connection(conn: &pulse_core::ConnectionRow) -> String {
    let local = format!("{}:{}", conn.local_addr, conn.local_port);
    let mut line = format!("{}  {local}", conn.protocol.label());
    // Listening sockets report a remote of 0.0.0.0:0.
    if let (Some(addr), Some(port)) = (conn.remote_addr, conn.remote_port)
        && (!addr.is_unspecified() || port != 0)
    {
        line.push_str(&format!(" -> {addr}:{port}"));
    }
    if let Some(state) = conn.state {
        line.push_str("  ");
        line.push_str(state);
    }
    line
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use pulse_core::{ConnectionRow, Protocol};

    use super::*;

    #[test]
    fn bytes_picks_a_sensible_unit() {
        assert_eq!(bytes(0), "0 B");
        assert_eq!(bytes(1023), "1023 B");
        assert_eq!(bytes(1024), "1.0 KB");
        assert_eq!(bytes(5 * 1024 * 1024 + 512 * 1024), "5.5 MB");
        assert_eq!(bytes(3 * 1024u64.pow(4)), "3.0 TB");
    }

    #[test]
    fn duration_drops_units_it_does_not_need() {
        assert_eq!(duration_secs(59), "0m 59s");
        assert_eq!(duration_secs(3 * 3600 + 5 * 60 + 7), "3h 05m 07s");
        assert_eq!(duration_secs(2 * 86_400 + 3600), "2d 01h 00m");
    }

    #[test]
    fn unknown_start_time_is_unavailable() {
        assert_eq!(started_at(0), "unavailable");
    }

    fn tcp(remote: [u8; 4], remote_port: u16, state: &'static str) -> ConnectionRow {
        ConnectionRow {
            pid: 1,
            protocol: Protocol::Tcp,
            local_addr: IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            local_port: 5000,
            remote_addr: Some(IpAddr::V4(remote.into())),
            remote_port: Some(remote_port),
            state: Some(state),
        }
    }

    #[test]
    fn established_connection_shows_remote() {
        assert_eq!(
            connection(&tcp([10, 0, 0, 2], 443, "Established")),
            "TCP  127.0.0.1:5000 -> 10.0.0.2:443  Established"
        );
    }

    #[test]
    fn listening_socket_hides_empty_remote() {
        assert_eq!(
            connection(&tcp([0, 0, 0, 0], 0, "Listening")),
            "TCP  127.0.0.1:5000  Listening"
        );
    }

    #[test]
    fn udp_has_no_remote_or_state() {
        let mut udp = tcp([0, 0, 0, 0], 0, "x");
        udp.protocol = Protocol::Udp;
        udp.remote_addr = None;
        udp.remote_port = None;
        udp.state = None;
        assert_eq!(connection(&udp), "UDP  127.0.0.1:5000");
    }
}
