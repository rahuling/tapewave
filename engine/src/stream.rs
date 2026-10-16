//! Bounded network prefetch. Socket reads never happen on the audio or command
//! thread. Dropping the stream cancels production; a silent server cannot keep
//! the decoder waiting indefinitely. A blocked OS read may finish later.

use std::io::{self, Cursor, Read, Seek, SeekFrom};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, mpsc};
use std::time::Duration;

struct Buffer {
    receiver: mpsc::Receiver<io::Result<Vec<u8>>>,
    chunk: Cursor<Vec<u8>>,
}

pub struct LiveStream {
    buffer: Mutex<Buffer>,
    cancelled: Arc<AtomicBool>,
}

impl LiveStream {
    pub fn new(mut reader: Box<dyn Read + Send>) -> Self {
        // At most 256 KiB of prefetched data per stream.
        let (tx, rx) = mpsc::sync_channel(8);
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancellation = Arc::clone(&cancelled);
        std::thread::spawn(move || {
            while !cancellation.load(Ordering::Relaxed) {
                let mut bytes = vec![0; 32 * 1024];
                match reader.read(&mut bytes) {
                    Ok(0) => break,
                    Ok(count) => {
                        bytes.truncate(count);
                        if tx.send(Ok(bytes)).is_err() {
                            break;
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(error) => {
                        let _ = tx.send(Err(error));
                        break;
                    }
                }
            }
        });
        Self {
            buffer: Mutex::new(Buffer {
                receiver: rx,
                chunk: Cursor::new(Vec::new()),
            }),
            cancelled,
        }
    }
}

impl Drop for LiveStream {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

impl Read for LiveStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let buffer = self
            .buffer
            .get_mut()
            .map_err(|_| io::Error::other("stream lock poisoned"))?;
        if buffer.chunk.position() as usize >= buffer.chunk.get_ref().len() {
            match buffer.receiver.recv_timeout(Duration::from_secs(10)) {
                Ok(Ok(chunk)) => buffer.chunk = Cursor::new(chunk),
                Ok(Err(error)) => return Err(error),
                Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(0),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "audio stream stalled for 10 seconds",
                    ));
                }
            }
        }
        buffer.chunk.read(buf)
    }
}

impl Seek for LiveStream {
    fn seek(&mut self, _pos: SeekFrom) -> io::Result<u64> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "live streams cannot seek",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_across_chunks_and_ends_at_eof() {
        let expected: Vec<u8> = (0..100_000).map(|i| (i % 251) as u8).collect();
        let mut stream = LiveStream::new(Box::new(Cursor::new(expected.clone())));
        let mut actual = Vec::new();
        stream.read_to_end(&mut actual).unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn seeking_is_explicitly_unsupported() {
        let mut stream = LiveStream::new(Box::new(Cursor::new(Vec::<u8>::new())));
        assert_eq!(
            stream.seek(SeekFrom::Start(0)).unwrap_err().kind(),
            io::ErrorKind::Unsupported
        );
    }

    #[test]
    fn reports_network_errors() {
        struct Broken;
        impl Read for Broken {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::new(io::ErrorKind::ConnectionReset, "reset"))
            }
        }
        let mut stream = LiveStream::new(Box::new(Broken));
        assert_eq!(
            stream.read(&mut [0; 1]).unwrap_err().kind(),
            io::ErrorKind::ConnectionReset
        );
    }
}
