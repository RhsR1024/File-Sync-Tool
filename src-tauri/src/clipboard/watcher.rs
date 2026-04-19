//! Clipboard listener (spec §5.1, §8.2). Implemented in M2.

pub struct WatcherHandle;

impl WatcherHandle {
    pub fn stop(self) {
        // TODO(M2): stop clipboard-master thread.
    }
}
