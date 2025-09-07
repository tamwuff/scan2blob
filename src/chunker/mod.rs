pub fn new(
    initial_chunk_size: usize,
    max_chunk_size: usize,
    max_num_chunks: usize,
) -> (Writer, Reader) {
    debug_assert!(initial_chunk_size > 0);
    debug_assert!(max_chunk_size > 0);
    debug_assert!(max_chunk_size >= initial_chunk_size);
    debug_assert!(max_num_chunks > 0);

    // The channels can hold 3 items each: 2 buffers, and a None.
    let (full_pending_read_writer, full_pending_read_reader) =
        tokio::sync::mpsc::channel::<Option<Vec<u8>>>(3);
    let (empty_pending_write_writer, empty_pending_write_reader) =
        tokio::sync::mpsc::channel::<Option<Vec<u8>>>(3);
    for _ in 0..2 {
        empty_pending_write_writer
            .try_send(Some(Vec::with_capacity(max_chunk_size)))
            .unwrap();
    }

    let result: std::sync::Arc<
        std::sync::OnceLock<Result<(), crate::error::WuffError>>,
    > = std::sync::Arc::new(std::sync::OnceLock::new());

    (
        Writer {
            chunk_size: initial_chunk_size,
            max_chunk_size,
            max_num_chunks,
            num_chunks: 0,
            done: false,
            result: std::sync::Arc::clone(&result),
            buf: None,
            full_pending_read: full_pending_read_writer,
            empty_pending_write: empty_pending_write_reader,
        },
        Reader {
            chunk: None,
            consumed_all_input: false,
            done: false,
            result,
            hasher: <md5::Md5 as md5::Digest>::new(),
            full_pending_read: full_pending_read_reader,
            empty_pending_write: empty_pending_write_writer,
        },
    )
}

// Invariants:
//
// 1. Nobody's allowed to drop any of the four MPSC channel halves, without
//    (first) setting `result`, and (second) enqueuing a None onto whichever
//    channel sender they hold. That means: (a) If there was an error that was
//    detected by the other side, you'll never be stuck waiting for a wakeup
//    that will never come. Even if you're awaiting on channel receive, you
//    will be awoken because you will get that None coming through on the
//    channel. (b) You'll never get a channel-closed error in practice when you
//    try to receive, because you'll always get a None first. (c) If you ever
//    get a channel-closed error when you try to send, you'll always be able to
//    see what the real error was.
// 2. If you get a None on a channel, that could be an error condition or it
//    could be because the other side is signaling that it is done and
//    successful. Distinguish between those two cases based on `result`. (If
//    `result` is unset, panic. That should never happen.)

pub struct Writer {
    chunk_size: usize,
    max_chunk_size: usize,
    max_num_chunks: usize,
    num_chunks: usize,
    done: bool,
    result: std::sync::Arc<
        std::sync::OnceLock<Result<(), crate::error::WuffError>>,
    >,
    buf: Option<Vec<u8>>,
    full_pending_read: tokio::sync::mpsc::Sender<Option<Vec<u8>>>,
    empty_pending_write: tokio::sync::mpsc::Receiver<Option<Vec<u8>>>,
}

impl Writer {
    pub async fn write(
        &mut self,
        mut buf: &[u8],
    ) -> Result<(), crate::error::WuffError> {
        self.sanity_check()?;

        while !buf.is_empty() {
            // Do we even have a buffer right now?
            let Some(dest_buf) = self.buf.as_mut() else {
                // No we don't. Can we try to get one, or would that make us
                // go over our max?
                if self.num_chunks >= self.max_num_chunks {
                    let error: crate::error::WuffError =
                        crate::error::WuffError::from(
                            "Exceeded maximum file size",
                        );
                    self.observe_error(error.clone());
                    return Err(error);
                }

                // OK, let's try to get a buffer. We unwrap() because of
                // invariant 1.
                let Some(fresh_buf) =
                    self.empty_pending_write.recv().await.unwrap()
                else {
                    return Err(self.other_side_exited());
                };

                // We've awaited since we last did a sanity check. Check again.
                self.sanity_check()?;

                // They promised us the buffer we got would be empty. Is it?
                debug_assert!(fresh_buf.is_empty());

                self.buf = Some(fresh_buf);
                self.num_chunks += 1;
                continue;
            };

            // Yes, we have a buffer! How full is it, relative to how full
            // we're willing to let it get? We panic if it's /too/ full. We
            // also panic if it's zero, because that means our previous
            // iteration should have enqueued it already.
            let bytes_avail_in_dest: usize =
                self.chunk_size.checked_sub(dest_buf.len()).unwrap();
            assert_ne!(bytes_avail_in_dest, 0);

            // How many bytes can we put in?
            let bytes_to_copy: &[u8];
            (bytes_to_copy, buf) =
                buf.split_at(std::cmp::min(buf.len(), bytes_avail_in_dest));
            dest_buf.extend_from_slice(bytes_to_copy);

            // Recalculate how full the buffer is, now. If it's exactly full,
            // time to ship it off.
            let bytes_avail_in_dest: usize =
                self.chunk_size.checked_sub(dest_buf.len()).unwrap();
            if bytes_avail_in_dest == 0 {
                let buf: Option<Vec<u8>> = self.buf.take();
                debug_assert!(buf.is_some());
                if let Err(_) = self.full_pending_read.try_send(buf) {
                    return Err(self.other_side_exited());
                }

                // Double the chunk size for next time.
                self.chunk_size =
                    std::cmp::min(self.chunk_size * 2, self.max_chunk_size);
            }
        }
        Ok(())
    }

    pub fn observe_error(&self, error: crate::error::WuffError) {
        let _ = self.result.set(Err(error));
        let _ = self.full_pending_read.try_send(None);
    }

    pub async fn finalize(&mut self) -> Result<(), crate::error::WuffError> {
        self.sanity_check()?;
        self.done = true;

        // Do we have one final buffer to send off?
        if let buf @ Some(_) = self.buf.take() {
            if let Err(_) = self.full_pending_read.try_send(buf) {
                return Err(self.other_side_exited());
            }
        }

        // Let them know we're done.
        if let Err(_) = self.full_pending_read.try_send(None) {
            return Err(self.other_side_exited());
        }

        // Wait for them to finish.
        for _ in 0..3 {
            // We unwrap() because of invariant 1.
            let theoretically_done =
                self.empty_pending_write.recv().await.unwrap().is_none();
            if let Some(result) = self.result.get() {
                return result.clone();
            } else if theoretically_done {
                panic!("Result not set");
            }
        }
        panic!("Infinite loop while trying to finalize writer");
    }

    // Use in situations where you have no particular reason to suspect
    // anything is wrong, but --
    //
    //   ... maybe an error has already happened...
    //
    //   ... maybe the whole thing has already succeeded, and we shouldn't be
    //   here...
    //
    //   ... or maybe our part of it is done, even if the whole thing is still
    //   in progress, and we still shouldn't be here.
    fn sanity_check(&self) -> Result<(), crate::error::WuffError> {
        match (self.done, self.result.get()) {
            (false, None) => Ok(()),
            (true, _) | (false, Some(Ok(()))) => {
                panic!("Attempted operation after done")
            }
            (false, Some(r @ Err(_))) => r.clone(),
        }
    }

    // Use in situations where you have reason to believe that the other side
    // has exited. (either we got a None on the channel, or else we tried to
    // send something and failed)
    fn other_side_exited(&self) -> crate::error::WuffError {
        match self.result.get() {
            None => panic!("Result not set"),
            Some(Ok(())) => {
                panic!("Result set to Ok with in-progress operation")
            }
            Some(Err(error)) => error.clone(),
        }
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        self.observe_error(crate::error::WuffError::from("Unexpected EOF"));
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ChunkOrEof<'a> {
    Chunk(&'a [u8]),
    Eof([u8; 16]),
}

pub struct Reader {
    consumed_all_input: bool,
    done: bool,
    result: std::sync::Arc<
        std::sync::OnceLock<Result<(), crate::error::WuffError>>,
    >,
    chunk: Option<Vec<u8>>,
    hasher: md5::Md5,
    full_pending_read: tokio::sync::mpsc::Receiver<Option<Vec<u8>>>,
    empty_pending_write: tokio::sync::mpsc::Sender<Option<Vec<u8>>>,
}

impl Reader {
    pub async fn get_next_chunk(
        &mut self,
    ) -> Result<ChunkOrEof, crate::error::WuffError> {
        assert!(!self.consumed_all_input, "Attempted operation after done");
        self.sanity_check()?;

        // If we were holding onto a chunk, release it back to the writer.
        if let Some(mut old_chunk) = self.chunk.take() {
            old_chunk.clear();
            if let Err(_) = self.empty_pending_write.try_send(Some(old_chunk))
            {
                self.other_side_exited()?;
                panic!("Result not set");
            }
        }

        // Try to get a fresh chunk. We unwrap() because of invariant 1.
        let Some(fresh_chunk) = self.full_pending_read.recv().await.unwrap()
        else {
            self.other_side_exited()?;
            self.consumed_all_input = true;
            let hash: [u8; 16] =
                <md5::Md5 as md5::Digest>::finalize(self.hasher.clone())
                    .as_slice()
                    .try_into()
                    .unwrap();
            return Ok(ChunkOrEof::Eof(hash));
        };

        <md5::Md5 as md5::Digest>::update(&mut self.hasher, &fresh_chunk);

        self.chunk = Some(fresh_chunk);
        Ok(ChunkOrEof::Chunk(self.chunk.as_ref().unwrap()))
    }

    pub fn observe_error(&self, error: crate::error::WuffError) {
        let _ = self.result.set(Err(error));
        let _ = self.empty_pending_write.try_send(None);
    }

    pub async fn finalize(&mut self) -> Result<(), crate::error::WuffError> {
        assert!(
            self.consumed_all_input,
            "Attempted finalize before all input consumed"
        );
        self.sanity_check()?;
        self.done = true;
        let result: Result<(), crate::error::WuffError> =
            self.result.get_or_init(|| Ok(())).clone();

        // Let them know we're done.
        if let Err(_) = self.empty_pending_write.try_send(None) {
            self.other_side_exited()?;
            panic!("Result not set");
        }

        result
    }

    // Use in situations where you have no particular reason to suspect
    // anything is wrong, but --
    //
    //   ... maybe an error has already happened...
    //
    //   ... maybe the whole thing has already succeeded, and we shouldn't be
    //   here...
    //
    //   ... or maybe our part of it is done, even if the whole thing is still
    //   in progress, and we still shouldn't be here.
    fn sanity_check(&self) -> Result<(), crate::error::WuffError> {
        match (self.done, self.result.get()) {
            (false, None) => Ok(()),
            (true, _) | (false, Some(Ok(()))) => {
                panic!("Attempted operation after done")
            }
            (false, Some(r @ Err(_))) => r.clone(),
        }
    }

    // Use in situations where you have reason to believe that the other side
    // has exited. (either we got a None on the channel, or else we tried to
    // send something and failed)
    fn other_side_exited(&self) -> Result<(), crate::error::WuffError> {
        match self.result.get() {
            None => Ok(()),
            Some(Ok(())) => {
                panic!("Result set to Ok with in-progress operation")
            }
            Some(error @ Err(_)) => error.clone(),
        }
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        self.observe_error(crate::error::WuffError::from("Reader went away"));
    }
}

#[cfg(test)]
mod test {
    use super::*;

    // md5 /dev/null
    const HASH_EMPTY: [u8; 16] = [
        0xd4u8, 0x1du8, 0x8cu8, 0xd9u8, 0x8fu8, 0x00u8, 0xb2u8, 0x04u8,
        0xe9u8, 0x80u8, 0x09u8, 0x98u8, 0xecu8, 0xf8u8, 0x42u8, 0x7eu8,
    ];
    // echo -n 'Hello, world!' | md5
    const HASH_HELLO_WORLD: [u8; 16] = [
        0x6cu8, 0xd3u8, 0x55u8, 0x6du8, 0xebu8, 0x0du8, 0xa5u8, 0x4bu8,
        0xcau8, 0x06u8, 0x0bu8, 0x4cu8, 0x39u8, 0x47u8, 0x98u8, 0x39u8,
    ];

    fn run_test<F>(f: F)
    where
        F: AsyncFn(std::sync::Arc<crate::ctx::Ctx>) -> (),
    {
        let ctx = std::sync::Arc::new(crate::ctx::Ctx::new());
        ctx.run_async_main({
            let ctx = std::sync::Arc::clone(&ctx);
            async move {
                f(ctx).await;
                Ok(())
            }
        })
        .expect("unexpected error");
    }

    #[test]
    fn write_zero_bytes_creates_no_chunks() {
        run_test(async |ctx| {
            let async_spawner = ctx.get_async_spawner();
            let (mut writer, mut reader) = new(1, 4, 10);

            let writer_task = async_spawner.spawn(async move {
                writer.finalize().await.unwrap();
            });

            let reader_task = async_spawner.spawn(async move {
                let chunk = reader.get_next_chunk().await.unwrap();
                assert_eq!(chunk, ChunkOrEof::Eof(HASH_EMPTY));

                reader.finalize().await.unwrap();
            });

            writer_task.await.unwrap();
            reader_task.await.unwrap();
        });
    }

    #[test]
    fn long_writes_get_split_up() {
        run_test(async |ctx| {
            let async_spawner = ctx.get_async_spawner();
            let (mut writer, mut reader) = new(1, 4, 10);

            let writer_task = async_spawner.spawn(async move {
                writer.write(b"Hello, world!").await.unwrap();
                writer.finalize().await.unwrap();
            });

            let reader_task = async_spawner.spawn(async move {
                let chunk = reader.get_next_chunk().await.unwrap();
                assert_eq!(chunk, ChunkOrEof::Chunk(b"H"));

                let chunk = reader.get_next_chunk().await.unwrap();
                assert_eq!(chunk, ChunkOrEof::Chunk(b"el"));

                let chunk = reader.get_next_chunk().await.unwrap();
                assert_eq!(chunk, ChunkOrEof::Chunk(b"lo, "));

                let chunk = reader.get_next_chunk().await.unwrap();
                assert_eq!(chunk, ChunkOrEof::Chunk(b"worl"));

                let chunk = reader.get_next_chunk().await.unwrap();
                assert_eq!(chunk, ChunkOrEof::Chunk(b"d!"));

                let chunk = reader.get_next_chunk().await.unwrap();
                assert_eq!(chunk, ChunkOrEof::Eof(HASH_HELLO_WORLD));

                reader.finalize().await.unwrap();
            });

            writer_task.await.unwrap();
            reader_task.await.unwrap();
        });
    }

    #[test]
    fn short_writes_get_aggregated() {
        run_test(async |ctx| {
            let async_spawner = ctx.get_async_spawner();
            let (mut writer, mut reader) = new(2, 8, 10);

            let writer_task = async_spawner.spawn(async move {
                for c in b"Hello, world!" {
                    writer.write(&[*c]).await.unwrap();
                }
                writer.finalize().await.unwrap();
            });

            let reader_task = async_spawner.spawn(async move {
                let chunk = reader.get_next_chunk().await.unwrap();
                assert_eq!(chunk, ChunkOrEof::Chunk(b"He"));

                let chunk = reader.get_next_chunk().await.unwrap();
                assert_eq!(chunk, ChunkOrEof::Chunk(b"llo,"));

                let chunk = reader.get_next_chunk().await.unwrap();
                assert_eq!(chunk, ChunkOrEof::Chunk(b" world!"));

                let chunk = reader.get_next_chunk().await.unwrap();
                assert_eq!(chunk, ChunkOrEof::Eof(HASH_HELLO_WORLD));

                reader.finalize().await.unwrap();
            });

            writer_task.await.unwrap();
            reader_task.await.unwrap();
        });
    }

    #[test]
    fn writer_goes_away_and_never_calls_finalize() {
        run_test(async |ctx| {
            let async_spawner = ctx.get_async_spawner();
            let (mut writer, mut reader) = new(2, 8, 10);

            let writer_task = async_spawner.spawn(async move {
                writer.write(b"Hello, world!").await.unwrap();
            });

            let reader_task = async_spawner.spawn(async move {
                let chunk = reader.get_next_chunk().await.unwrap();
                assert_eq!(chunk, ChunkOrEof::Chunk(b"He"));

                let chunk = reader.get_next_chunk().await.unwrap();
                assert_eq!(chunk, ChunkOrEof::Chunk(b"llo,"));

                let result = reader.get_next_chunk().await;
                assert!(result.is_err());
            });

            writer_task.await.unwrap();
            reader_task.await.unwrap();
        });
    }

    #[test]
    fn reader_reads_data_but_goes_away_before_finalize() {
        run_test(async |ctx| {
            let async_spawner = ctx.get_async_spawner();
            let (mut writer, mut reader) = new(2, 8, 10);

            let writer_task = async_spawner.spawn(async move {
                writer.write(b"Hello, world!").await.unwrap();

                let result = writer.finalize().await;
                assert!(result.is_err());
            });

            let reader_task = async_spawner.spawn(async move {
                let chunk = reader.get_next_chunk().await.unwrap();
                assert_eq!(chunk, ChunkOrEof::Chunk(b"He"));

                let chunk = reader.get_next_chunk().await.unwrap();
                assert_eq!(chunk, ChunkOrEof::Chunk(b"llo,"));

                let chunk = reader.get_next_chunk().await.unwrap();
                assert_eq!(chunk, ChunkOrEof::Chunk(b" world!"));

                let chunk = reader.get_next_chunk().await.unwrap();
                assert_eq!(chunk, ChunkOrEof::Eof(HASH_HELLO_WORLD));
            });

            writer_task.await.unwrap();
            reader_task.await.unwrap();
        });
    }

    #[test]
    fn reader_goes_away_before_fully_reading_data() {
        run_test(async |ctx| {
            let async_spawner = ctx.get_async_spawner();
            let (mut writer, mut reader) = new(2, 8, 10);

            let writer_task = async_spawner.spawn(async move {
                let result = writer.write(b"Hello, world!").await;
                assert!(result.is_err());
            });

            let reader_task = async_spawner.spawn(async move {
                let chunk = reader.get_next_chunk().await.unwrap();
                assert_eq!(chunk, ChunkOrEof::Chunk(b"He"));
            });

            writer_task.await.unwrap();
            reader_task.await.unwrap();
        });
    }

    #[test]
    fn exactly_max_chunk_count_is_ok() {
        run_test(async |ctx| {
            let async_spawner = ctx.get_async_spawner();
            let (mut writer, mut reader) = new(2, 4, 4);

            let writer_task = async_spawner.spawn(async move {
                writer.write(b"Hello, world!").await.unwrap();
                writer.finalize().await.unwrap();
            });

            let reader_task = async_spawner.spawn(async move {
                let chunk = reader.get_next_chunk().await.unwrap();
                assert_eq!(chunk, ChunkOrEof::Chunk(b"He"));

                let chunk = reader.get_next_chunk().await.unwrap();
                assert_eq!(chunk, ChunkOrEof::Chunk(b"llo,"));

                let chunk = reader.get_next_chunk().await.unwrap();
                assert_eq!(chunk, ChunkOrEof::Chunk(b" wor"));

                let chunk = reader.get_next_chunk().await.unwrap();
                assert_eq!(chunk, ChunkOrEof::Chunk(b"ld!"));

                let chunk = reader.get_next_chunk().await.unwrap();
                assert_eq!(chunk, ChunkOrEof::Eof(HASH_HELLO_WORLD));

                reader.finalize().await.unwrap();
            });

            writer_task.await.unwrap();
            reader_task.await.unwrap();
        });
    }

    #[test]
    fn one_more_than_max_chunk_count_is_not_ok() {
        run_test(async |ctx| {
            let async_spawner = ctx.get_async_spawner();
            let (mut writer, mut reader) = new(2, 4, 3);

            let writer_task = async_spawner.spawn(async move {
                let result = writer.write(b"Hello, world!").await;
                assert!(result.is_err());
            });

            let reader_task = async_spawner.spawn(async move {
                for _ in 0..10 {
                    let result = reader.get_next_chunk().await;
                    if result.is_err() {
                        // Good!
                        return;
                    }
                }
                panic!("Got too many chunks and never saw an error");
            });

            writer_task.await.unwrap();
            reader_task.await.unwrap();
        });
    }
}
