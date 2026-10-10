//! Adapts a live HTTP body to the `Read + Seek` bound rodio's decoder requires.
//! Live streams cannot seek, so `seek` always fails and the decoder is told
//! not to rely on it.

use std::io::{self, Read, Seek, SeekFrom};
use std::sync::Mutex;

pub struct LiveStream {
    // Mutex makes the reader Sync, which rodio requires. ureq's body reader is
    // Send but not Sync.
    inner: Mutex<Box<dyn Read + Send>>,
}

impl LiveStream {
    pub fn new(reader: Box<dyn Read + Send>) -> Self {
        Self { inner: Mutex::new(reader) }
    }
}

impl Read for LiveStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner
            .lock()
            .map_err(|_| io::Error::other("stream lock poisoned"))?
            .read(buf)
    }
}

impl Seek for LiveStream {
    fn seek(&mut self, _pos: SeekFrom) -> io::Result<u64> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "live streams cannot seek"))
    }
}
