use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::device_simulator::media::Codec;

pub const RTP_HEADER_BYTES: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RtpPacketizer {
    pub payload_type: u8,
    pub ssrc: u32,
    pub next_sequence: u16,
    pub max_payload_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RtpPacket {
    pub sequence: u16,
    pub timestamp: u32,
    pub marker: bool,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RtpPacketError {
    pub code: &'static str,
    pub message: String,
}

impl RtpPacketizer {
    pub fn packetize_access_unit(
        &mut self,
        codec: Codec,
        nals: &[&[u8]],
        timestamp: u32,
    ) -> Result<Vec<RtpPacket>, RtpPacketError> {
        match codec {
            Codec::H264 => self.packetize_h264_access_unit(nals, timestamp),
            Codec::H265 => self.packetize_h265_access_unit(nals, timestamp),
        }
    }

    pub fn packetize_h264_access_unit(
        &mut self,
        nals: &[&[u8]],
        timestamp: u32,
    ) -> Result<Vec<RtpPacket>, RtpPacketError> {
        if !(96..=127).contains(&self.payload_type) || self.max_payload_bytes < 3 || nals.is_empty()
        {
            return Err(error(
                "device_simulator.rtp.configuration_invalid",
                "invalid RTP packetizer configuration",
            ));
        }
        let mut packets = Vec::new();
        for (nal_index, nal) in nals.iter().enumerate() {
            if nal.is_empty() {
                return Err(error(
                    "device_simulator.rtp.nal_invalid",
                    "H.264 NAL unit is empty",
                ));
            }
            let access_unit_end = nal_index + 1 == nals.len();
            if nal.len() <= self.max_payload_bytes {
                packets.push(self.make_packet(nal, timestamp, access_unit_end));
                continue;
            }
            let indicator = (nal[0] & 0xe0) | 28;
            let nal_type = nal[0] & 0x1f;
            let fragment_size = self.max_payload_bytes - 2;
            let fragments = nal[1..].chunks(fragment_size).collect::<Vec<_>>();
            for (index, fragment) in fragments.iter().enumerate() {
                let start = index == 0;
                let end = index + 1 == fragments.len();
                let fu_header = (u8::from(start) << 7) | (u8::from(end) << 6) | nal_type;
                let mut payload = Vec::with_capacity(fragment.len() + 2);
                payload.push(indicator);
                payload.push(fu_header);
                payload.extend_from_slice(fragment);
                packets.push(self.make_packet(&payload, timestamp, access_unit_end && end));
            }
        }
        Ok(packets)
    }

    pub fn packetize_h265_access_unit(
        &mut self,
        nals: &[&[u8]],
        timestamp: u32,
    ) -> Result<Vec<RtpPacket>, RtpPacketError> {
        if !(96..=127).contains(&self.payload_type) || self.max_payload_bytes < 4 || nals.is_empty()
        {
            return Err(error(
                "device_simulator.rtp.configuration_invalid",
                "invalid RTP packetizer configuration",
            ));
        }
        let mut packets = Vec::new();
        for (nal_index, nal) in nals.iter().enumerate() {
            if nal.len() < 2 {
                return Err(error(
                    "device_simulator.rtp.nal_invalid",
                    "H.265 NAL unit has an incomplete two-byte header",
                ));
            }
            let access_unit_end = nal_index + 1 == nals.len();
            if nal.len() <= self.max_payload_bytes {
                packets.push(self.make_packet(nal, timestamp, access_unit_end));
                continue;
            }

            let original_type = (nal[0] >> 1) & 0x3f;
            let fu_indicator_first = (nal[0] & 0x81) | (49 << 1);
            let fu_indicator_second = nal[1];
            let fragment_size = self.max_payload_bytes - 3;
            let fragments = nal[2..].chunks(fragment_size).collect::<Vec<_>>();
            for (index, fragment) in fragments.iter().enumerate() {
                let start = index == 0;
                let end = index + 1 == fragments.len();
                let fu_header = (u8::from(start) << 7) | (u8::from(end) << 6) | original_type;
                let mut payload = Vec::with_capacity(fragment.len() + 3);
                payload.extend_from_slice(&[fu_indicator_first, fu_indicator_second, fu_header]);
                payload.extend_from_slice(fragment);
                packets.push(self.make_packet(&payload, timestamp, access_unit_end && end));
            }
        }
        Ok(packets)
    }

    fn make_packet(&mut self, payload: &[u8], timestamp: u32, marker: bool) -> RtpPacket {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.wrapping_add(1);
        let mut bytes = Vec::with_capacity(RTP_HEADER_BYTES + payload.len());
        bytes.push(0x80);
        bytes.push((u8::from(marker) << 7) | self.payload_type);
        bytes.extend_from_slice(&sequence.to_be_bytes());
        bytes.extend_from_slice(&timestamp.to_be_bytes());
        bytes.extend_from_slice(&self.ssrc.to_be_bytes());
        bytes.extend_from_slice(payload);
        RtpPacket {
            sequence,
            timestamp,
            marker,
            bytes,
        }
    }
}

pub fn tcp_interleaved_frame(channel: u8, rtp_packet: &[u8]) -> Result<Vec<u8>, RtpPacketError> {
    let length = u16::try_from(rtp_packet.len()).map_err(|_| {
        error(
            "device_simulator.rtp.packet_too_large",
            "RTP packet is too large for RTSP interleaved framing",
        )
    })?;
    let mut frame = Vec::with_capacity(rtp_packet.len() + 4);
    frame.extend_from_slice(&[b'$', channel]);
    frame.extend_from_slice(&length.to_be_bytes());
    frame.extend_from_slice(rtp_packet);
    Ok(frame)
}

/// Builds an RFC 3550 RTCP Sender Report for the RTP stream identified by
/// `ssrc`. The report deliberately uses the timestamp of the most recently
/// queued RTP packet so the NTP/RTP clock mapping describes the same media
/// point the receiver has just seen.
pub fn rtcp_sender_report(
    ssrc: u32,
    rtp_timestamp: u32,
    sender_packet_count: u32,
    sender_octet_count: u32,
    now: SystemTime,
) -> [u8; 28] {
    let duration = now.duration_since(UNIX_EPOCH).unwrap_or_default();
    let ntp_seconds = duration.as_secs().saturating_add(2_208_988_800);
    let ntp_fraction = ((u64::from(duration.subsec_nanos()) << 32) / 1_000_000_000) as u32;
    let mut report = [0u8; 28];
    report[0] = 0x80; // Version 2, no padding, zero report blocks.
    report[1] = 200; // SR packet type.
    report[2..4].copy_from_slice(&6u16.to_be_bytes());
    report[4..8].copy_from_slice(&ssrc.to_be_bytes());
    report[8..12].copy_from_slice(&(ntp_seconds as u32).to_be_bytes());
    report[12..16].copy_from_slice(&ntp_fraction.to_be_bytes());
    report[16..20].copy_from_slice(&rtp_timestamp.to_be_bytes());
    report[20..24].copy_from_slice(&sender_packet_count.to_be_bytes());
    report[24..28].copy_from_slice(&sender_octet_count.to_be_bytes());
    report
}

/// RFC 3550 requires an SR/RR to be sent as part of a compound RTCP packet
/// containing an SDES CNAME. The legacy camera capture follows that layout, and
/// some recorders discard a standalone SR even though its 28-byte body parses.
pub fn rtcp_compound_sender_report(
    ssrc: u32,
    rtp_timestamp: u32,
    sender_packet_count: u32,
    sender_octet_count: u32,
    now: SystemTime,
    cname: &[u8],
) -> Result<Vec<u8>, RtpPacketError> {
    if cname.is_empty() || cname.len() > u8::MAX as usize {
        return Err(error(
            "device_simulator.rtcp.cname_invalid",
            "RTCP SDES CNAME must contain 1..=255 bytes",
        ));
    }

    let sender_report = rtcp_sender_report(
        ssrc,
        rtp_timestamp,
        sender_packet_count,
        sender_octet_count,
        now,
    );
    let unpadded_sdes_len = 4 + 4 + 2 + cname.len() + 1;
    let sdes_len = unpadded_sdes_len.div_ceil(4) * 4;
    let length_words_minus_one = u16::try_from(sdes_len / 4 - 1).map_err(|_| {
        error(
            "device_simulator.rtcp.sdes_too_large",
            "RTCP SDES packet is too large",
        )
    })?;
    let mut compound = Vec::with_capacity(sender_report.len() + sdes_len);
    compound.extend_from_slice(&sender_report);
    compound.extend_from_slice(&[0x81, 202]); // Version 2, one SDES chunk.
    compound.extend_from_slice(&length_words_minus_one.to_be_bytes());
    compound.extend_from_slice(&ssrc.to_be_bytes());
    compound.extend_from_slice(&[1, cname.len() as u8]); // CNAME item.
    compound.extend_from_slice(cname);
    compound.push(0); // End of SDES items.
    compound.resize(sender_report.len() + sdes_len, 0);
    Ok(compound)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaClock {
    timestamp: u32,
    clock_rate: u32,
    frames_per_second: u32,
    remainder: u32,
}

impl MediaClock {
    pub fn new(
        timestamp: u32,
        clock_rate: u32,
        frames_per_second: u32,
    ) -> Result<Self, RtpPacketError> {
        if clock_rate == 0 || frames_per_second == 0 || frames_per_second > clock_rate {
            return Err(error(
                "device_simulator.rtp.clock_invalid",
                "media clock rate/fps is invalid",
            ));
        }
        Ok(Self {
            timestamp,
            clock_rate,
            frames_per_second,
            remainder: 0,
        })
    }

    pub fn current(&self) -> u32 {
        self.timestamp
    }

    pub fn advance_frame(&mut self) -> u32 {
        let base = self.clock_rate / self.frames_per_second;
        self.remainder = self
            .remainder
            .saturating_add(self.clock_rate % self.frames_per_second);
        let extra = if self.remainder >= self.frames_per_second {
            self.remainder -= self.frames_per_second;
            1
        } else {
            0
        };
        self.timestamp = self.timestamp.wrapping_add(base + extra);
        self.timestamp
    }
}

#[derive(Debug)]
pub struct BoundedClientQueue<T> {
    capacity: usize,
    queue: VecDeque<T>,
}

impl<T> BoundedClientQueue<T> {
    pub fn new(capacity: usize) -> Result<Self, RtpPacketError> {
        if capacity == 0 {
            return Err(error(
                "device_simulator.rtp.queue_invalid",
                "client queue capacity must be non-zero",
            ));
        }
        Ok(Self {
            capacity,
            queue: VecDeque::with_capacity(capacity),
        })
    }

    pub fn try_push(&mut self, item: T) -> Result<(), T> {
        if self.queue.len() == self.capacity {
            return Err(item);
        }
        self.queue.push_back(item);
        Ok(())
    }

    pub fn pop_front(&mut self) -> Option<T> {
        self.queue.pop_front()
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }
}

fn error(code: &'static str, message: impl Into<String>) -> RtpPacketError {
    RtpPacketError {
        code,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packetizes_single_and_fragmented_h264_with_continuous_sequence() {
        let mut packetizer = RtpPacketizer {
            payload_type: 105,
            ssrc: 0x01020304,
            next_sequence: u16::MAX,
            max_payload_bytes: 6,
        };
        let packets = packetizer
            .packetize_h264_access_unit(&[&[0x65, 1, 2], &[0x61, 3, 4, 5, 6, 7, 8, 9]], 90_000)
            .unwrap();
        assert_eq!(packets[0].sequence, u16::MAX);
        assert_eq!(packets[1].sequence, 0);
        assert!(!packets[0].marker);
        assert!(packets.last().unwrap().marker);
        assert_eq!(packets[1].bytes[RTP_HEADER_BYTES] & 0x1f, 28);
        assert_ne!(packets[1].bytes[RTP_HEADER_BYTES + 1] & 0x80, 0);
        assert_ne!(
            packets.last().unwrap().bytes[RTP_HEADER_BYTES + 1] & 0x40,
            0
        );
    }

    #[test]
    fn packetizes_single_and_fragmented_h265_with_wrap() {
        let mut packetizer = RtpPacketizer {
            payload_type: 105,
            ssrc: 0x01020304,
            next_sequence: u16::MAX,
            max_payload_bytes: 7,
        };
        let packets = packetizer
            .packetize_access_unit(
                Codec::H265,
                &[&[0x26, 0x01, 1], &[0x02, 0x01, 2, 3, 4, 5, 6, 7, 8, 9]],
                u32::MAX,
            )
            .unwrap();
        assert_eq!(packets[0].sequence, u16::MAX);
        assert_eq!(packets[1].sequence, 0);
        assert_eq!((packets[1].bytes[RTP_HEADER_BYTES] >> 1) & 0x3f, 49);
        assert_ne!(packets[1].bytes[RTP_HEADER_BYTES + 2] & 0x80, 0);
        assert_ne!(
            packets.last().unwrap().bytes[RTP_HEADER_BYTES + 2] & 0x40,
            0
        );
        assert!(packets.last().unwrap().marker);
    }

    #[test]
    fn wraps_timestamps_and_builds_interleaved_frames() {
        let mut clock = MediaClock::new(u32::MAX - 100, 90_000, 25).unwrap();
        assert_eq!(clock.advance_frame(), 3_499);
        let packet = vec![1, 2, 3];
        assert_eq!(
            tcp_interleaved_frame(2, &packet).unwrap(),
            vec![b'$', 2, 0, 3, 1, 2, 3]
        );
    }

    #[test]
    fn builds_rfc3550_sender_report_with_ntp_and_sender_counters() {
        let report = rtcp_sender_report(
            0x0102_0304,
            90_000,
            12,
            34_567,
            UNIX_EPOCH + std::time::Duration::new(1, 500_000_000),
        );
        assert_eq!(report[0], 0x80);
        assert_eq!(report[1], 200);
        assert_eq!(&report[2..4], &6u16.to_be_bytes());
        assert_eq!(&report[4..8], &0x0102_0304u32.to_be_bytes());
        assert_eq!(&report[8..12], &2_208_988_801u32.to_be_bytes());
        assert_eq!(&report[12..16], &0x8000_0000u32.to_be_bytes());
        assert_eq!(&report[16..20], &90_000u32.to_be_bytes());
        assert_eq!(&report[20..24], &12u32.to_be_bytes());
        assert_eq!(&report[24..28], &34_567u32.to_be_bytes());
    }

    #[test]
    fn builds_compound_sender_report_with_sdes_cname() {
        let compound = rtcp_compound_sender_report(
            0x0102_0304,
            90_000,
            12,
            34_567,
            UNIX_EPOCH + std::time::Duration::from_secs(1),
            b"file-sync-tool@virtual-device",
        )
        .unwrap();
        assert_eq!(&compound[..4], &[0x80, 200, 0, 6]);
        assert_eq!(&compound[28..30], &[0x81, 202]);
        let sdes_len = (usize::from(u16::from_be_bytes([compound[30], compound[31]])) + 1) * 4;
        assert_eq!(compound.len(), 28 + sdes_len);
        assert_eq!(&compound[32..36], &0x0102_0304u32.to_be_bytes());
        assert_eq!(compound[36], 1);
        let cname_len = compound[37] as usize;
        assert_eq!(
            &compound[38..38 + cname_len],
            b"file-sync-tool@virtual-device"
        );
        assert!(compound[38 + cname_len..].iter().all(|byte| *byte == 0));
    }

    #[test]
    fn slow_client_queue_is_bounded_without_dropping_existing_packets() {
        let mut queue = BoundedClientQueue::new(2).unwrap();
        queue.try_push(1).unwrap();
        queue.try_push(2).unwrap();
        assert_eq!(queue.try_push(3), Err(3));
        assert_eq!(queue.len(), 2);
        assert_eq!(queue.pop_front(), Some(1));
    }
}
