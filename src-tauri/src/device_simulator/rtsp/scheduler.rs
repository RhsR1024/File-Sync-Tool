use super::rtp::{MediaClock, RtpPacketError};
use crate::device_simulator::media::SharedMediaPack;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, watch};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedNal {
    buffer: Arc<[u8]>,
    offset: usize,
    length: usize,
}

impl SharedNal {
    pub fn from_bytes(bytes: impl Into<Arc<[u8]>>) -> Self {
        let buffer = bytes.into();
        let length = buffer.len();
        Self {
            buffer,
            offset: 0,
            length,
        }
    }

    fn from_shared_buffer(buffer: Arc<[u8]>, offset: usize, length: usize) -> Self {
        Self {
            buffer,
            offset,
            length,
        }
    }
}

impl AsRef<[u8]> for SharedNal {
    fn as_ref(&self) -> &[u8] {
        &self.buffer[self.offset..self.offset + self.length]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedAccessUnit {
    pub nals: Arc<[SharedNal]>,
    pub keyframe: bool,
}

#[derive(Debug, Clone)]
pub struct ScheduledAccessUnit {
    pub frame_index: usize,
    pub timestamp: u32,
    pub access_unit: Arc<SharedAccessUnit>,
}

#[derive(Debug, Clone)]
pub struct SharedFrameScheduler {
    frames: Arc<[Arc<SharedAccessUnit>]>,
    sender: broadcast::Sender<ScheduledAccessUnit>,
    clock_rate: u32,
    frame_duration_ticks: u32,
}

impl SharedFrameScheduler {
    pub fn new(
        frames: Arc<[Arc<SharedAccessUnit>]>,
        clock_rate: u32,
        frames_per_second: u32,
        client_queue_capacity: usize,
    ) -> Result<Self, RtpPacketError> {
        if frames.is_empty() || frames.iter().any(|frame| frame.nals.is_empty()) {
            return Err(RtpPacketError {
                code: "device_simulator.rtsp.frames_invalid",
                message: "shared frame sequence is empty or contains an empty access unit".into(),
            });
        }
        MediaClock::new(0, clock_rate, frames_per_second)?;
        if clock_rate % frames_per_second != 0 {
            return Err(RtpPacketError {
                code: "device_simulator.rtsp.frame_duration_inexact",
                message: "use a loaded media pack for fractional frame rates".into(),
            });
        }
        Self::with_duration(
            frames,
            clock_rate,
            clock_rate / frames_per_second,
            client_queue_capacity,
        )
    }

    pub fn from_media(
        media: Arc<SharedMediaPack>,
        client_queue_capacity: usize,
    ) -> Result<Self, RtpPacketError> {
        let buffer = media.shared_bytes();
        let frames = media
            .frames()
            .iter()
            .map(|frame| {
                Arc::new(SharedAccessUnit {
                    nals: frame
                        .nals
                        .iter()
                        .map(|nal| {
                            SharedNal::from_shared_buffer(
                                Arc::clone(&buffer),
                                nal.offset,
                                nal.length,
                            )
                        })
                        .collect::<Vec<_>>()
                        .into(),
                    keyframe: frame.keyframe,
                })
            })
            .collect::<Vec<_>>();
        let frame_duration_ticks = media
            .frames()
            .first()
            .map(|frame| frame.duration_ticks)
            .ok_or(RtpPacketError {
                code: "device_simulator.rtsp.frames_invalid",
                message: "loaded media has no frames".into(),
            })?;
        if media
            .frames()
            .iter()
            .any(|frame| frame.duration_ticks != frame_duration_ticks)
        {
            return Err(RtpPacketError {
                code: "device_simulator.rtsp.variable_frame_duration_unsupported",
                message: "the shared scheduler requires a constant indexed frame duration".into(),
            });
        }
        Self::with_duration(
            frames.into(),
            media.manifest().clock_rate,
            frame_duration_ticks,
            client_queue_capacity,
        )
    }

    fn with_duration(
        frames: Arc<[Arc<SharedAccessUnit>]>,
        clock_rate: u32,
        frame_duration_ticks: u32,
        client_queue_capacity: usize,
    ) -> Result<Self, RtpPacketError> {
        if frames.is_empty()
            || frames.iter().any(|frame| frame.nals.is_empty())
            || clock_rate == 0
            || frame_duration_ticks == 0
            || frame_duration_ticks > clock_rate
        {
            return Err(RtpPacketError {
                code: "device_simulator.rtsp.frames_invalid",
                message: "shared media frames or clock are invalid".into(),
            });
        }
        if client_queue_capacity == 0 || client_queue_capacity > 4_096 {
            return Err(RtpPacketError {
                code: "device_simulator.rtsp.queue_invalid",
                message: "client queue capacity must be within 1..=4096".into(),
            });
        }
        let (sender, _) = broadcast::channel(client_queue_capacity);
        Ok(Self {
            frames,
            sender,
            clock_rate,
            frame_duration_ticks,
        })
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ScheduledAccessUnit> {
        self.sender.subscribe()
    }

    pub fn frames(&self) -> &[Arc<SharedAccessUnit>] {
        &self.frames
    }

    pub fn clock_rate(&self) -> u32 {
        self.clock_rate
    }

    pub fn spawn(&self, mut shutdown: watch::Receiver<bool>) -> tokio::task::JoinHandle<()> {
        let frames = Arc::clone(&self.frames);
        let sender = self.sender.clone();
        let clock_rate = self.clock_rate;
        let frame_duration_ticks = self.frame_duration_ticks;
        tokio::spawn(async move {
            let period =
                Duration::from_secs_f64(f64::from(frame_duration_ticks) / f64::from(clock_rate));
            let mut interval = tokio::time::interval(period);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut timestamp = 0_u32;
            let mut frame_index = 0usize;
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        let frame = ScheduledAccessUnit {
                            frame_index,
                            timestamp,
                            access_unit: Arc::clone(&frames[frame_index]),
                        };
                        // No receivers is normal. A slow receiver is isolated by
                        // the bounded broadcast queue and reports lag to itself.
                        let _ = sender.send(frame);
                        frame_index = (frame_index + 1) % frames.len();
                        timestamp = timestamp.wrapping_add(frame_duration_ticks);
                    }
                    changed = shutdown.changed() => {
                        if changed.is_err() || *shutdown.borrow() {
                            break;
                        }
                    }
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frames() -> Arc<[Arc<SharedAccessUnit>]> {
        vec![
            Arc::new(SharedAccessUnit {
                nals: vec![SharedNal::from_bytes([0x65, 1, 2].as_slice())].into(),
                keyframe: true,
            }),
            Arc::new(SharedAccessUnit {
                nals: vec![SharedNal::from_bytes([0x61, 3, 4].as_slice())].into(),
                keyframe: false,
            }),
        ]
        .into()
    }

    #[tokio::test]
    async fn scheduler_loops_without_resetting_shared_timestamp() {
        let scheduler = SharedFrameScheduler::new(frames(), 90_000, 25, 2).unwrap();
        let mut receiver = scheduler.subscribe();
        let (stop, stop_rx) = watch::channel(false);
        let task = scheduler.spawn(stop_rx);
        let first = receiver.recv().await.unwrap();
        let second = receiver.recv().await.unwrap();
        let third = receiver.recv().await.unwrap();
        assert_eq!(
            (first.frame_index, second.frame_index, third.frame_index),
            (0, 1, 0)
        );
        assert_eq!(
            (first.timestamp, second.timestamp, third.timestamp),
            (0, 3_600, 7_200)
        );
        stop.send(true).unwrap();
        task.await.unwrap();
    }

    #[tokio::test]
    async fn slow_subscriber_lags_without_blocking_another_subscriber() {
        let scheduler = SharedFrameScheduler::new(frames(), 90_000, 1_000, 1).unwrap();
        let mut slow = scheduler.subscribe();
        let mut current = scheduler.subscribe();
        for index in 0..3 {
            scheduler
                .sender
                .send(ScheduledAccessUnit {
                    frame_index: index % 2,
                    timestamp: index as u32 * 90,
                    access_unit: Arc::clone(&scheduler.frames[index % 2]),
                })
                .unwrap();
            assert_eq!(current.recv().await.unwrap().timestamp, index as u32 * 90);
        }
        assert!(matches!(
            slow.recv().await,
            Err(broadcast::error::RecvError::Lagged(_))
        ));
    }
}
