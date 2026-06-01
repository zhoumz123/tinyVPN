use anyhow::Result;
use std::net::SocketAddr;

/// Discover our public IP:port using STUN
/// MVP: sends a raw STUN Binding Request to a public STUN server
pub async fn discover_public_endpoint() -> Result<SocketAddr> {
    // For MVP, we'll do a simple approach: send a UDP packet to the CCS
    // and let it tell us our public address.
    // Production version should use proper STUN (RFC 5389).

    // Simple STUN-like discovery via external service
    let stun_addr = "stun.l.google.com:19302";

    let socket = tokio::net::UdpSocket::bind("0.0.0.0:0").await?;
    socket.connect(stun_addr).await?;

    // STUN Binding Request (RFC 5389)
    // Message Type: 0x0001 (Binding Request)
    // Message Length: 0x0000
    // Magic Cookie: 0x2112A442
    // Transaction ID: 12 random bytes
    let mut request = vec![0x00, 0x01, 0x00, 0x00, 0x21, 0x12, 0xA4, 0x42];
    let mut txn_id = [0u8; 12];
    rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut txn_id);
    request.extend_from_slice(&txn_id);

    socket.send(&request).await?;

    let mut buf = [0u8; 256];
    let n = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        socket.recv(&mut buf),
    )
    .await??;

    if n < 20 {
        anyhow::bail!("STUN response too short");
    }

    // Parse STUN response for XOR-MAPPED-ADDRESS (attribute type 0x0020)
    parse_xor_mapped_address(&buf[..n], &txn_id)
}

fn parse_xor_mapped_address(data: &[u8], _txn_id: &[u8; 12]) -> Result<SocketAddr> {
    // STUN header: 20 bytes, then attributes
    let msg_len = ((data[2] as u16) << 8) | data[3] as u16;
    let attrs_end = 20 + msg_len as usize;
    let attrs = &data[20..attrs_end.min(data.len())];

    let mut offset = 0;
    while offset + 4 <= attrs.len() {
        let attr_type = ((attrs[offset] as u16) << 8) | attrs[offset + 1] as u16;
        let attr_len = ((attrs[offset + 2] as u16) << 8) | attrs[offset + 3] as u16;

        if attr_type == 0x0020 {
            // XOR-MAPPED-ADDRESS
            let val = &attrs[offset + 4..offset + 4 + attr_len as usize];
            if val.len() < 4 {
                anyhow::bail!("XOR-MAPPED-ADDRESS too short");
            }
            let family = val[1];
            let port = (((val[2] as u16) << 8) | val[3] as u16) ^ 0x2112;

            return match family {
                0x01 => {
                    // IPv4
                    let ip_bytes = &val[4..8];
                    let ip = std::net::Ipv4Addr::new(
                        ip_bytes[0] ^ 0x21,
                        ip_bytes[1] ^ 0x12,
                        ip_bytes[2] ^ 0xA4,
                        ip_bytes[3] ^ 0x42,
                    );
                    Ok(SocketAddr::new(std::net::IpAddr::V4(ip), port))
                }
                _ => anyhow::bail!("Unsupported address family: {}", family),
            };
        }
        offset += 4 + attr_len as usize;
        // Attributes are padded to 4-byte boundary
        offset = (offset + 3) & !3;
    }

    anyhow::bail!("No XOR-MAPPED-ADDRESS found in STUN response")
}
