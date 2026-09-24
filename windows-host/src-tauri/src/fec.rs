//! Two original datagrams plus one XOR repair datagram. Originals never wait.
//! Envelope: RWFX, version=1, reserved=0, original length(u16), session(u64),
//! first frame(u64), XOR of both complete original RWAV datagrams. Big endian.
pub const HEADER: usize = 24;
pub const MAX: usize = HEADER + 1024;

#[derive(Default)]
pub struct Encoder {
    first: Option<(u64, usize, [u8; 1024])>,
}
impl Encoder {
    pub fn push(
        &mut self,
        session: u64,
        frame: u64,
        packet: &[u8],
        out: &mut [u8; MAX],
    ) -> Option<usize> {
        if !matches!(packet.len(), 266 | 468 | 544 | 1024) {
            self.first = None;
            return None;
        }
        if let Some((base, len, first)) = self.first.take() {
            if frame == base + 240 && packet.len() == len && packet[6] == first[6] {
                out[..4].copy_from_slice(b"RWFX");
                out[4] = 1;
                out[5] = 0;
                out[6..8].copy_from_slice(&(len as u16).to_be_bytes());
                out[8..16].copy_from_slice(&session.to_be_bytes());
                out[16..24].copy_from_slice(&base.to_be_bytes());
                for i in 0..len {
                    out[HEADER + i] = first[i] ^ packet[i];
                }
                return Some(HEADER + len);
            }
        }
        let mut first = [0; 1024];
        first[..packet.len()].copy_from_slice(packet);
        self.first = Some((frame, packet.len(), first));
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repairs_either_original_exactly_and_stays_under_mtu() {
        for len in [266, 468, 544, 1024] {
            let a = vec![17u8; len];
            let mut b = vec![219u8; len];
            b[6] = a[6];
            let mut e = Encoder::default();
            let mut out = [0; MAX];
            assert!(e.push(42, 0, &a, &mut out).is_none());
            let n = e.push(42, 240, &b, &mut out).unwrap();
            assert_eq!(n, len + 24);
            assert!(n + 48 <= 1280);
            assert_eq!(
                &out[..8],
                &[82, 87, 70, 88, 1, 0, (len >> 8) as u8, len as u8]
            );
            assert_eq!(u64::from_be_bytes(out[8..16].try_into().unwrap()), 42);
            for i in 0..len {
                assert_eq!(out[24 + i] ^ a[i], b[i]);
                assert_eq!(out[24 + i] ^ b[i], a[i]);
            }
        }
    }
    #[test]
    fn gaps_and_format_changes_restart_pair() {
        let mut e = Encoder::default();
        let mut out = [0; MAX];
        assert!(e.push(1, 0, &[0; 544], &mut out).is_none());
        assert!(e.push(1, 480, &[0; 544], &mut out).is_none());
        assert!(e.push(1, 720, &[0; 1024], &mut out).is_none());
        assert_eq!(e.push(1, 960, &[0; 1024], &mut out), Some(1048));
        assert_eq!(u64::from_be_bytes(out[16..24].try_into().unwrap()), 720);
    }
}
