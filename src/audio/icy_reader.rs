use icy_metadata::{error::MetadataParseError, IcyMetadata};
use std::collections::VecDeque;
use std::fmt::Debug;
use std::io::{self, Read, Seek, SeekFrom};
use std::num::{NonZero, NonZeroUsize};

const ICY_METADATA_MULTIPLIER: usize = 16;

/// ICY reader that correctly handles partial reads from network streams.
pub struct IcyMetadataReader<T> {
    inner: T,
    metadata_interval: Option<usize>,
    next_metadata: usize,
    metadata_sizes: MetadataSizeQueue,
    current_pos: u64,
    on_metadata_read: Box<dyn Fn(Result<IcyMetadata, MetadataParseError>) + Send + Sync>,
}

impl<T> Debug for IcyMetadataReader<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("IcyMetadataReader")
            .field("inner", &"<inner>")
            .field("metadata_interval", &self.metadata_interval)
            .field("next_metadata", &self.next_metadata)
            .field("metadata_sizes", &self.metadata_sizes)
            .field("current_pos", &self.current_pos)
            .field("on_metadata_read", &"<on_metadata_read>")
            .finish()
    }
}

impl<T> IcyMetadataReader<T> {
    pub fn new<F>(inner: T, metadata_interval: Option<NonZeroUsize>, on_metadata_read: F) -> Self
    where
        F: Fn(Result<IcyMetadata, MetadataParseError>) + Send + Sync + 'static,
    {
        let metadata_interval = metadata_interval.map(NonZero::get);
        Self {
            inner,
            metadata_interval,
            next_metadata: metadata_interval.unwrap_or(0),
            metadata_sizes: MetadataSizeQueue::default(),
            current_pos: 0,
            on_metadata_read: Box::new(on_metadata_read),
        }
    }

    fn read_metadata(&mut self) -> io::Result<()>
    where
        T: Read,
    {
        let mut length = [0_u8; 1];
        self.inner.read_exact(&mut length)?;
        let length = length[0] as usize * ICY_METADATA_MULTIPLIER;
        self.metadata_sizes.push(length);

        if length == 0 {
            return Ok(());
        }

        let mut metadata = vec![0_u8; length];
        self.inner.read_exact(&mut metadata)?;
        let parsed = String::from_utf8(metadata)
            .map_err(MetadataParseError::InvalidUtf8)
            .and_then(|value| {
                value
                    .trim_end_matches(char::from(0))
                    .parse::<IcyMetadata>()
                    .map_err(MetadataParseError::Empty)
            });
        (self.on_metadata_read)(parsed);
        Ok(())
    }
}

impl<T: Read> Read for IcyMetadataReader<T> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }

        let Some(metadata_interval) = self.metadata_interval else {
            return self.inner.read(buffer);
        };

        if self.next_metadata == 0 {
            self.read_metadata()?;
            self.next_metadata = metadata_interval;
        }

        let limit = buffer.len().min(self.next_metadata);
        let read = self.inner.read(&mut buffer[..limit])?;
        self.next_metadata -= read;
        self.current_pos += read as u64;
        Ok(read)
    }
}

impl<T: Read + Seek> Seek for IcyMetadataReader<T> {
    fn seek(&mut self, seek_from: SeekFrom) -> io::Result<u64> {
        let Some(metadata_interval) = self.metadata_interval else {
            return self.inner.seek(seek_from);
        };

        let (requested_change, requested_pos) = match seek_from {
            SeekFrom::Start(pos) => (pos as i64 - self.current_pos as i64, pos as i64),
            SeekFrom::Current(change) => (change, self.current_pos as i64 + change),
            SeekFrom::End(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "seek from end not supported",
                ));
            }
        };

        if requested_pos < 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "cannot seek before start of stream",
            ));
        }

        let mut absolute_pos = self.inner.stream_position()? as i64;
        let mut seek_progress = 0_i64;

        if requested_change < 0 {
            let mut last_metadata_offset = (metadata_interval - self.next_metadata) as i64;
            let mut last_metadata_end = absolute_pos - last_metadata_offset;

            while absolute_pos + requested_change - seek_progress < last_metadata_end
                && last_metadata_end > 0
            {
                let Some(last_metadata_size) = self.metadata_sizes.pop() else {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "cannot seek beyond ICY metadata cache",
                    ));
                };
                let metadata_region_size = last_metadata_size as i64 + 1;
                absolute_pos = self.inner.seek(SeekFrom::Start(
                    (last_metadata_end - metadata_region_size) as u64,
                ))? as i64;
                seek_progress -= last_metadata_offset;
                last_metadata_offset = metadata_interval as i64;
                last_metadata_end -= metadata_region_size + metadata_interval as i64;
            }
        } else {
            while requested_change - seek_progress >= self.next_metadata as i64 {
                self.inner
                    .seek(SeekFrom::Current(self.next_metadata as i64))?;
                seek_progress += self.next_metadata as i64;
                self.read_metadata()?;
                self.next_metadata = metadata_interval;
            }
        }

        self.inner
            .seek(SeekFrom::Current(requested_change - seek_progress))?;
        self.next_metadata = metadata_interval - requested_pos as usize % metadata_interval;
        self.current_pos = requested_pos as u64;
        Ok(self.current_pos)
    }
}

#[derive(Debug)]
struct MetadataSize {
    size: usize,
    count: usize,
}

#[derive(Debug)]
struct MetadataSizeQueue {
    entries: VecDeque<MetadataSize>,
    capacity: usize,
}

impl Default for MetadataSizeQueue {
    fn default() -> Self {
        Self {
            entries: VecDeque::new(),
            capacity: 128,
        }
    }
}

impl MetadataSizeQueue {
    fn push(&mut self, size: usize) {
        if let Some(last) = self.entries.back_mut() {
            if last.size == size {
                last.count += 1;
                return;
            }
        }

        self.entries.push_back(MetadataSize { size, count: 1 });
        if self.entries.len() >= self.capacity {
            self.entries.pop_front();
        }
    }

    fn pop(&mut self) -> Option<usize> {
        let last = self.entries.back_mut()?;
        last.count -= 1;
        let size = last.size;
        if last.count == 0 {
            self.entries.pop_back();
        }
        Some(size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::sync::{Arc, Mutex};

    struct ShortReader {
        inner: Cursor<Vec<u8>>,
        max_read: usize,
    }

    impl Read for ShortReader {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            let limit = buffer.len().min(self.max_read);
            self.inner.read(&mut buffer[..limit])
        }
    }

    impl Seek for ShortReader {
        fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
            self.inner.seek(position)
        }
    }

    #[test]
    fn partial_reads_do_not_cross_metadata_boundary() {
        const METAINT: usize = 49_152;
        const TRAILING_AUDIO: usize = 32_768;

        let metadata = "StreamTitle='Deep ambient';";
        let metadata_blocks = metadata.len().div_ceil(ICY_METADATA_MULTIPLIER);
        let mut stream = vec![1_u8; METAINT];
        stream.push(metadata_blocks as u8);
        stream.extend_from_slice(metadata.as_bytes());
        stream.resize(
            stream.len() + metadata_blocks * ICY_METADATA_MULTIPLIER - metadata.len(),
            0,
        );
        stream.extend(vec![2_u8; TRAILING_AUDIO]);

        let titles = Arc::new(Mutex::new(Vec::new()));
        let captured_titles = titles.clone();
        let inner = ShortReader {
            inner: Cursor::new(stream),
            max_read: 7_000,
        };
        let mut reader =
            IcyMetadataReader::new(inner, NonZeroUsize::new(METAINT), move |metadata| {
                if let Ok(metadata) = metadata {
                    captured_titles
                        .lock()
                        .unwrap()
                        .push(metadata.stream_title().unwrap().to_string());
                }
            });

        let mut audio = Vec::new();
        reader.read_to_end(&mut audio).unwrap();

        assert_eq!(audio.len(), METAINT + TRAILING_AUDIO);
        assert!(audio[..METAINT].iter().all(|byte| *byte == 1));
        assert!(audio[METAINT..].iter().all(|byte| *byte == 2));
        assert_eq!(*titles.lock().unwrap(), vec!["Deep ambient"]);
    }
}
