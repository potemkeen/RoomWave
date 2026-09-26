use crate::layout::channel_index;

use super::{AudioBlock, FRAMES};

#[cfg(test)]
use crate::layout::{test_sample, Layout};

#[cfg(test)]
use super::PCM_BYTES;

pub(super) struct RoutedPacket {
    pub(super) frame: u64,
    pub(super) play_ns: u64,
    pub(super) bytes: [u8; 1024],
    pub(super) len: usize,
}

fn extended_packet(session: u64, block: &AudioBlock, send_ns: u64) -> [u8; 1024] {
    let mut result = [0u8; 1024];

    result[..8].copy_from_slice(b"RWAV\x04\x01\x02\x01");
    result[8..16].copy_from_slice(&session.to_be_bytes());
    result[16..20].copy_from_slice(&((block.frame / FRAMES as u64) as u32).to_be_bytes());
    result[20..28].copy_from_slice(&block.frame.to_be_bytes());
    result[28..30].copy_from_slice(&(FRAMES as u16).to_be_bytes());
    result[32..40].copy_from_slice(&block.capture_ns.to_be_bytes());
    result[40..48].copy_from_slice(&block.play_ns.to_be_bytes());
    result[48..56].copy_from_slice(&send_ns.to_be_bytes());
    result[56..64].copy_from_slice(&block.read_ns.to_be_bytes());
    result[64..].copy_from_slice(&block.pcm);

    result
}

pub(super) fn routed_packet(
    session: u64,
    block: &AudioBlock,
    send_ns: u64,
    speaker: Option<u32>,
) -> RoutedPacket {
    let mut bytes = extended_packet(session, block, send_ns);

    let len = if let Some(speaker) = speaker {
        bytes[6] = 1;

        let index = channel_index(block.mask, speaker).filter(|index| *index < block.channels);

        for frame in 0..FRAMES {
            let value = index
                .map(|index| block.source[frame * block.channels + index])
                .unwrap_or(0);

            bytes[64 + frame * 2..66 + frame * 2].copy_from_slice(&value.to_le_bytes());
        }

        544
    } else {
        // A speaker test addresses explicitly assigned devices only.
        if block.test_channel.is_some() {
            bytes[64..].fill(0);
        }

        1024
    };

    RoutedPacket {
        frame: block.frame,
        play_ns: block.play_ns,
        bytes,
        len,
    }
}

#[cfg(test)]
fn packet(session: u64, block: &AudioBlock) -> Vec<u8> {
    let mut result = Vec::with_capacity(48 + PCM_BYTES);

    result.extend_from_slice(b"RWAV");
    result.extend_from_slice(&[4, 1, 2, 0]);
    result.extend_from_slice(&session.to_be_bytes());
    result.extend_from_slice(&((block.frame / FRAMES as u64) as u32).to_be_bytes());
    result.extend_from_slice(&block.frame.to_be_bytes());
    result.extend_from_slice(&(FRAMES as u16).to_be_bytes());
    result.extend_from_slice(&[0, 0]);
    result.extend_from_slice(&block.capture_ns.to_be_bytes());
    result.extend_from_slice(&block.play_ns.to_be_bytes());
    result.extend_from_slice(&block.pcm);

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mono_routing_preserves_samples_deadlines_and_isolates_test() {
        for (channels, mask) in [(2, 3), (6, 0x3f), (6, 0x60f), (8, 0x63f)] {
            let mut block = AudioBlock {
                frame: 240,
                capture_ns: 10,
                read_ns: 20,
                published_ns: 0,
                play_ns: 100,
                pcm: [9; PCM_BYTES],
                source: [0; FRAMES * 8],
                mode: 0,
                mask,
                channels,
                test_channel: None,
            };

            for n in 0..FRAMES {
                for c in 0..channels {
                    block.source[n * channels + c] = (n as i16) * 8 + c as i16;
                }
            }

            for speaker in Layout::new(channels, mask).channels {
                let wire = routed_packet(1, &block, 30, Some(speaker.mask));

                assert_eq!(wire.len, 544);
                assert_eq!(wire.bytes[6], 1);

                assert_eq!(
                    u64::from_be_bytes(wire.bytes[40..48].try_into().unwrap()),
                    100
                );

                for n in 0..FRAMES {
                    assert_eq!(
                        i16::from_le_bytes(wire.bytes[64 + n * 2..66 + n * 2].try_into().unwrap()),
                        n as i16 * 8 + speaker.index as i16
                    );
                }
            }

            let unavailable = routed_packet(1, &block, 30, Some(0x80000000));

            assert!(unavailable.bytes[64..544].iter().all(|value| *value == 0));

            assert_eq!(&routed_packet(1, &block, 30, None).bytes[64..], &block.pcm);

            block.test_channel = Some(1);
            block.source.fill(0);

            for n in 0..FRAMES {
                block.source[n * channels] = test_sample(n as u64 + 1000, 48000, 1);
            }

            assert!(routed_packet(1, &block, 30, Some(1)).bytes[64..544]
                .iter()
                .any(|value| *value != 0));

            assert!(routed_packet(1, &block, 30, Some(2)).bytes[64..544]
                .iter()
                .all(|value| *value == 0));

            assert!(routed_packet(1, &block, 30, None).bytes[64..]
                .iter()
                .all(|value| *value == 0));
        }
    }

    #[test]
    fn wire_packet_preserves_common_timeline_after_sequence_wrap() {
        let block = AudioBlock {
            frame: (u32::MAX as u64 + 6) * 240,
            capture_ns: 1234567890123,
            read_ns: 1234567890123,
            published_ns: 0,
            play_ns: 1235067890123,
            pcm: [7; PCM_BYTES],
            source: [0; FRAMES * 8],
            mode: 0,
            mask: 3,
            channels: 2,
            test_channel: None,
        };

        let a = packet(123, &block);
        let b = packet(456, &block);

        assert_eq!(a.len(), 1008);
        assert_eq!(&a[..8], b"RWAV\x04\x01\x02\x00");

        assert_eq!(u32::from_be_bytes(a[16..20].try_into().unwrap()), 5);

        assert_eq!(
            u64::from_be_bytes(a[20..28].try_into().unwrap()),
            block.frame
        );

        assert_eq!(
            u64::from_be_bytes(a[40..48].try_into().unwrap()),
            block.play_ns
        );

        assert_eq!(&a[16..], &b[16..]);
        assert_ne!(&a[8..16], &b[8..16]);
    }
}
