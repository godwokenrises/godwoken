use std::io::{self, Write};

use zstd::{
    stream::Encoder,
    zstd_safe::{get_error_name, DCtx, InBuffer, OutBuffer},
};

// 16 MiB.
const MAX_FRAME_SIZE: usize = 16 * 1024 * 1024;

pub struct StreamEncoder<'a> {
    encoder: Encoder<'a, Vec<u8>>,
}

impl<'a> StreamEncoder<'a> {
    /// Create a new StreamEncoder with the specific compression level. See zstd
    /// compression levels.
    pub fn new(level: i32) -> io::Result<Self> {
        Ok(Self {
            encoder: Encoder::new(Vec::new(), level)?,
        })
    }

    pub fn encode(&mut self, chunk: &[u8]) -> io::Result<Vec<u8>> {
        if chunk.len() > MAX_FRAME_SIZE {
            return Err(io::ErrorKind::OutOfMemory.into());
        }

        self.encoder.write_all(chunk)?;
        self.encoder.flush()?;

        Ok(std::mem::take(self.encoder.get_mut()))
    }
}

#[derive(Default)]
pub struct StreamDecoder {
    // Use low level API for decompression, because the flush API of the
    // streaming API does not seem to do what I want.
    decoder: DCtx<'static>,
}

impl StreamDecoder {
    pub fn new() -> Self {
        Self {
            decoder: DCtx::create(),
        }
    }

    pub fn decode(&mut self, compressed_chunk: &[u8]) -> io::Result<Vec<u8>> {
        if compressed_chunk.is_empty() {
            return Ok(Vec::new());
        }

        let mut result = Vec::new();
        let mut input = InBuffer::around(compressed_chunk);
        let mut output = OutBuffer::around(&mut result);
        // Make sure to consume all input and get all output.
        //
        // input.pos == input.len means all input is consumed.
        //
        // output.pos < output.capacity means that there is no more output.
        while input.pos() < input.src.len() || output.pos() == output.dst.capacity() {
            if output.pos() > MAX_FRAME_SIZE {
                return Err(io::ErrorKind::OutOfMemory.into());
            }
            let new_capacity = std::cmp::min(
                if output.dst.capacity() == 0 {
                    compressed_chunk.len()
                } else {
                    output.dst.capacity()
                } * 2,
                MAX_FRAME_SIZE + 1,
            );
            output.dst.reserve(new_capacity);
            self.decoder
                .decompress_stream(&mut output, &mut input)
                .map_err(|code| io::Error::new(io::ErrorKind::Other, get_error_name(code)))?;
        }
        result.shrink_to_fit();
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use rand::{thread_rng, Rng, RngCore};

    use super::*;

    #[test]
    fn test_compress_and_decompress() -> io::Result<()> {
        let mut enc = StreamEncoder::new(3)?;
        let mut dec = StreamDecoder::new();

        for len in [0, 1, 128, 1024, 4096, 8192, 16384, MAX_FRAME_SIZE] {
            test_len(len, &mut dec, &mut enc)?;
        }

        for _ in 0..100 {
            let len = thread_rng().gen_range(0..128 * 1024);
            test_len(len, &mut dec, &mut enc)?;
        }

        Ok(())
    }

    fn test_len(
        len: usize,
        dec: &mut StreamDecoder,
        enc: &mut StreamEncoder,
    ) -> Result<(), io::Error> {
        let msg = {
            let mut result = vec![0u8; len];
            thread_rng().fill_bytes(&mut result);
            result
        };
        let result = dec.decode(&enc.encode(&msg)?)?;
        assert_eq!(result.len(), msg.len());
        assert_eq!(result, &*msg);
        let result = dec.decode(&enc.encode(&msg)?)?;
        assert_eq!(result.len(), msg.len());
        assert_eq!(result, &*msg);
        Ok(())
    }
}
