use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use windows::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, NO_ERROR};
use windows::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, GetExtendedUdpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCPROW_OWNER_PID,
    MIB_UDP6ROW_OWNER_PID, MIB_UDPROW_OWNER_PID, TCP_TABLE_OWNER_PID_ALL, UDP_TABLE_OWNER_PID,
};
use windows::Win32::Networking::WinSock::{AF_INET, AF_INET6};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Protocol {
    Tcp,
    Udp,
}

impl Protocol {
    pub fn label(self) -> &'static str {
        match self {
            Protocol::Tcp => "TCP",
            Protocol::Udp => "UDP",
        }
    }
}

#[derive(Clone, Debug)]
pub struct ConnectionRow {
    pub pid: u32,
    pub protocol: Protocol,
    pub local_addr: IpAddr,
    pub local_port: u16,
    pub remote_addr: Option<IpAddr>,
    pub remote_port: Option<u16>,
    /// None for UDP.
    pub state: Option<&'static str>,
}

/// All TCP and UDP sockets, IPv4 and IPv6. A table that fails to load (e.g.
/// IPv6 disabled) is skipped rather than taking the others down with it.
pub fn list_connections() -> Vec<ConnectionRow> {
    let v4 = AF_INET.0 as u32;
    let v6 = AF_INET6.0 as u32;
    [
        tcp_connections(v4),
        tcp_connections(v6),
        udp_connections(v4),
        udp_connections(v6),
    ]
    .into_iter()
    .flatten()
    .flatten()
    .collect()
}

fn tcp_state_name(state: u32) -> &'static str {
    match state {
        1 => "Closed",
        2 => "Listening",
        3 => "Syn sent",
        4 => "Syn received",
        5 => "Established",
        6 => "Fin wait 1",
        7 => "Fin wait 2",
        8 => "Close wait",
        9 => "Closing",
        10 => "Last ack",
        11 => "Time wait",
        12 => "Delete TCB",
        _ => "Unknown",
    }
}

fn ipv4_from_be(raw: u32) -> Ipv4Addr {
    Ipv4Addr::from(u32::from_be(raw))
}

fn port_from_be(raw: u32) -> u16 {
    u16::from_be(raw as u16)
}

fn tcp_connections(address_family: u32) -> anyhow::Result<Vec<ConnectionRow>> {
    let buf = fetch_table(|ptr, size| unsafe {
        GetExtendedTcpTable(ptr, size, false, address_family, TCP_TABLE_OWNER_PID_ALL, 0)
    })?;

    if address_family == AF_INET.0 as u32 {
        let rows: Vec<MIB_TCPROW_OWNER_PID> = unsafe { decode_rows(&buf) };
        Ok(rows
            .into_iter()
            .map(|row| ConnectionRow {
                pid: row.dwOwningPid,
                protocol: Protocol::Tcp,
                local_addr: IpAddr::V4(ipv4_from_be(row.dwLocalAddr)),
                local_port: port_from_be(row.dwLocalPort),
                remote_addr: Some(IpAddr::V4(ipv4_from_be(row.dwRemoteAddr))),
                remote_port: Some(port_from_be(row.dwRemotePort)),
                state: Some(tcp_state_name(row.dwState)),
            })
            .collect())
    } else {
        let rows: Vec<MIB_TCP6ROW_OWNER_PID> = unsafe { decode_rows(&buf) };
        Ok(rows
            .into_iter()
            .map(|row| ConnectionRow {
                pid: row.dwOwningPid,
                protocol: Protocol::Tcp,
                local_addr: IpAddr::V6(Ipv6Addr::from(row.ucLocalAddr)),
                local_port: port_from_be(row.dwLocalPort),
                remote_addr: Some(IpAddr::V6(Ipv6Addr::from(row.ucRemoteAddr))),
                remote_port: Some(port_from_be(row.dwRemotePort)),
                state: Some(tcp_state_name(row.dwState)),
            })
            .collect())
    }
}

fn udp_connections(address_family: u32) -> anyhow::Result<Vec<ConnectionRow>> {
    let buf = fetch_table(|ptr, size| unsafe {
        GetExtendedUdpTable(ptr, size, false, address_family, UDP_TABLE_OWNER_PID, 0)
    })?;

    if address_family == AF_INET.0 as u32 {
        let rows: Vec<MIB_UDPROW_OWNER_PID> = unsafe { decode_rows(&buf) };
        Ok(rows
            .into_iter()
            .map(|row| ConnectionRow {
                pid: row.dwOwningPid,
                protocol: Protocol::Udp,
                local_addr: IpAddr::V4(ipv4_from_be(row.dwLocalAddr)),
                local_port: port_from_be(row.dwLocalPort),
                remote_addr: None,
                remote_port: None,
                state: None,
            })
            .collect())
    } else {
        let rows: Vec<MIB_UDP6ROW_OWNER_PID> = unsafe { decode_rows(&buf) };
        Ok(rows
            .into_iter()
            .map(|row| ConnectionRow {
                pid: row.dwOwningPid,
                protocol: Protocol::Udp,
                local_addr: IpAddr::V6(Ipv6Addr::from(row.ucLocalAddr)),
                local_port: port_from_be(row.dwLocalPort),
                remote_addr: None,
                remote_port: None,
                state: None,
            })
            .collect())
    }
}

// The table can grow between the sizing call and the real one, hence the
// retries. Vec<u32> keeps the buffer 4-byte aligned for decode_rows.
fn fetch_table(
    mut query: impl FnMut(Option<*mut core::ffi::c_void>, &mut u32) -> u32,
) -> anyhow::Result<Vec<u32>> {
    let mut size: u32 = 0;
    let sizing_status = query(None, &mut size);
    if sizing_status != ERROR_INSUFFICIENT_BUFFER.0 && sizing_status != NO_ERROR.0 {
        anyhow::bail!("network table size query failed with status {sizing_status}");
    }
    if size == 0 {
        return Ok(Vec::new());
    }

    for _ in 0..3 {
        let words = (size as usize).div_ceil(4);
        let mut buf = vec![0u32; words];
        let mut actual_size = size;
        let status = query(Some(buf.as_mut_ptr().cast()), &mut actual_size);
        match status {
            s if s == NO_ERROR.0 => return Ok(buf),
            s if s == ERROR_INSUFFICIENT_BUFFER.0 => size = actual_size,
            other => anyhow::bail!("network table query failed with status {other}"),
        }
    }
    anyhow::bail!("network table kept growing while reading it")
}

/// Every MIB_*TABLE_OWNER_PID is a u32 count followed by that many rows.
///
/// # Safety
/// `buf` must come from `fetch_table` on the query that returns `Row`s.
unsafe fn decode_rows<Row: Copy>(buf: &[u32]) -> Vec<Row> {
    if buf.is_empty() {
        return Vec::new();
    }
    let num_entries = buf[0] as usize;
    let available_bytes = buf.len() * 4;
    let max_rows = available_bytes.saturating_sub(4) / core::mem::size_of::<Row>();
    let n = num_entries.min(max_rows);

    let rows_ptr = unsafe { (buf.as_ptr() as *const u8).add(4) as *const Row };
    unsafe { std::slice::from_raw_parts(rows_ptr, n) }.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Values as they sit in the table: network byte order in memory.
    fn be_addr(octets: [u8; 4]) -> u32 {
        u32::from_ne_bytes(octets)
    }

    fn be_port(port: u16) -> u32 {
        u16::from_ne_bytes(port.to_be_bytes()) as u32
    }

    #[test]
    fn converts_network_order_address_and_port() {
        assert_eq!(
            ipv4_from_be(be_addr([127, 0, 0, 1])),
            Ipv4Addr::new(127, 0, 0, 1)
        );
        assert_eq!(port_from_be(be_port(443)), 443);
        assert_eq!(port_from_be(be_port(51820)), 51820);
    }

    #[test]
    fn decodes_tcp4_table() {
        let row = MIB_TCPROW_OWNER_PID {
            dwState: 5,
            dwLocalAddr: be_addr([192, 168, 1, 5]),
            dwLocalPort: be_port(51820),
            dwRemoteAddr: be_addr([93, 184, 216, 34]),
            dwRemotePort: be_port(443),
            dwOwningPid: 1234,
        };
        let fields = [
            row.dwState,
            row.dwLocalAddr,
            row.dwLocalPort,
            row.dwRemoteAddr,
            row.dwRemotePort,
            row.dwOwningPid,
        ];
        let mut buf = vec![2u32];
        buf.extend(fields);
        buf.extend(fields);

        let rows: Vec<MIB_TCPROW_OWNER_PID> = unsafe { decode_rows(&buf) };
        assert_eq!(rows, [row, row]);
        assert_eq!(tcp_state_name(rows[0].dwState), "Established");
    }

    #[test]
    fn decode_never_reads_past_the_buffer() {
        // Claims 50 rows but only holds one.
        let mut buf = vec![50u32];
        buf.extend([5, 0, 0, 0, 0, 7]);
        let rows: Vec<MIB_TCPROW_OWNER_PID> = unsafe { decode_rows(&buf) };
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].dwOwningPid, 7);
    }

    #[test]
    fn decode_handles_empty_buffer() {
        let rows: Vec<MIB_UDPROW_OWNER_PID> = unsafe { decode_rows(&[]) };
        assert!(rows.is_empty());
    }
}
