use std::ops::Range;

// GPU buffers rotate independently. Keep a bounded CPU shadow for each physical slot, rather
// than comparing with the previous frame (which may have used a different buffer).
const MAX_SHADOW_BYTES: usize = 2 * 1024 * 1024;
const MAX_SLOTS: usize = 8;
const BLOCK_BYTES: usize = 256;
const MAX_WRITES: usize = 32;

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct UploadStats {
    pub bytes: u64,
    pub writes: usize,
    pub reused: usize,
    pub shadow_bytes: usize,
}

#[derive(Default)]
pub(crate) struct BufferUploads {
    previous: [Vec<u8>; MAX_SLOTS],
    stats: UploadStats,
}

impl BufferUploads {
    pub(crate) fn begin_frame(&mut self) {
        self.stats = UploadStats::default();
    }

    pub(crate) fn reset(&mut self, slot: usize) {
        self.previous[slot].clear();
    }

    pub(crate) fn stats(&self) -> UploadStats {
        UploadStats {
            shadow_bytes: self.previous.iter().map(Vec::capacity).sum(),
            ..self.stats
        }
    }

    pub(crate) fn write(
        &mut self,
        slot: usize,
        queue: &wgpu::Queue,
        buffer: &wgpu::Buffer,
        data: &[u8],
    ) {
        debug_assert_eq!(data.len() % wgpu::COPY_BUFFER_ALIGNMENT as usize, 0);
        let previous = &mut self.previous[slot];
        if data.len() > MAX_SHADOW_BYTES {
            // A large working set must not silently become a second unbounded CPU cache.
            *previous = Vec::new();
            queue.write_buffer(buffer, 0, data);
            self.stats.bytes += data.len() as u64;
            self.stats.writes += 1;
            return;
        }
        let ranges = DirtyRanges::between(previous, data);
        if data.len() > previous.capacity() {
            previous.reserve_exact(data.len() - previous.len());
        }
        previous.resize(data.len(), 0);
        if ranges.len == 0 {
            self.stats.reused += 1;
        }
        for range in &ranges.ranges[..ranges.len] {
            queue.write_buffer(buffer, range.start as u64, &data[range.clone()]);
            previous[range.clone()].copy_from_slice(&data[range.clone()]);
            self.stats.bytes += range.len() as u64;
            self.stats.writes += 1;
        }
    }
}

struct DirtyRanges {
    ranges: [Range<usize>; MAX_WRITES],
    len: usize,
}

impl DirtyRanges {
    fn between(previous: &[u8], current: &[u8]) -> Self {
        let mut result = Self {
            ranges: std::array::from_fn(|_| 0..0),
            len: 0,
        };
        for (index, block) in current.chunks(BLOCK_BYTES).enumerate() {
            let start = index * BLOCK_BYTES;
            let end = start + block.len();
            if previous.get(start..end) == Some(block) {
                continue;
            }
            if result.len > 0
                && (result.ranges[result.len - 1].end == start || result.len == MAX_WRITES)
            {
                result.ranges[result.len - 1].end = end;
            } else {
                result.ranges[result.len] = start..end;
                result.len += 1;
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dirty_uploads_cover_edits_growth_and_truncation_with_bounded_aligned_writes() {
        let mut previous = vec![0; 32768];
        for len in [32768, 65536, 1024, 0, 4096] {
            let mut next = vec![1; len];
            for i in (0..len).step_by(512) {
                next[i..(i + 256).min(len)].fill(0);
            }
            let ranges = DirtyRanges::between(&previous, &next);
            assert!(ranges.len <= MAX_WRITES);
            previous.resize(len, 0);
            for range in &ranges.ranges[..ranges.len] {
                assert_eq!(range.start % 4, 0);
                assert_eq!(range.end % 4, 0);
                previous[range.clone()].copy_from_slice(&next[range.clone()]);
            }
            assert_eq!(previous, next);
            assert_eq!(DirtyRanges::between(&previous, &next).len, 0);
        }
    }

    #[test]
    fn one_changed_instance_does_not_upload_an_unchanged_megabyte() {
        let previous = vec![0; 1024 * 1024];
        let mut next = previous.clone();
        next[512] = 1;
        let ranges = DirtyRanges::between(&previous, &next);
        assert_eq!(ranges.len, 1);
        assert_eq!(ranges.ranges[0], 512..768);
    }
}
