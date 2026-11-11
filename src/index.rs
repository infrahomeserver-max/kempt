//! Line index over a memory-mapped file. Built by a background thread so a
//! viewer can open instantly and scroll/search while indexing continues.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

pub struct Shared {
    /// start offset of every line seen so far
    pub starts: Mutex<Vec<u64>>,
    /// bytes scanned so far
    pub scanned: AtomicU64,
    pub done: AtomicBool,
}

#[derive(Clone)]
pub struct LineIndex {
    pub sh: Arc<Shared>,
}

/// Scan `data[from..]`, appending line starts to `out`.
/// A line start is recorded after each '\n' unless it is end-of-data.
fn scan_into(data: &[u8], from: usize, out: &mut Vec<u64>) {
    for p in memchr::memchr_iter(b'\n', &data[from..]) {
        let next = from + p + 1;
        if next < data.len() {
            out.push(next as u64);
        }
    }
}

/// Synchronous full scan (used by diff).
pub fn line_starts(data: &[u8]) -> Vec<u64> {
    let mut v = Vec::with_capacity(data.len() / 64 + 1);
    if !data.is_empty() {
        v.push(0);
    }
    scan_into(data, 0, &mut v);
    v
}

impl LineIndex {
    pub fn new() -> Self {
        LineIndex {
            sh: Arc::new(Shared { starts: Mutex::new(Vec::new()), scanned: AtomicU64::new(0), done: AtomicBool::new(false) }),
        }
    }

    /// Scan `data` in the background in 4 MiB slices, publishing progress.
    pub fn spawn(&self, data: Arc<memmap2::Mmap>) -> std::thread::JoinHandle<()> {
        let sh = self.sh.clone();
        std::thread::spawn(move || {
            let len = data.len();
            if len > 0 {
                sh.starts.lock().unwrap().push(0);
            }
            let mut pos = 0usize;
            while pos < len {
                let end = (pos + (4 << 20)).min(len);
                // find newlines whose *next-line start* lies in [pos, end]
                let mut local = Vec::new();
                for p in memchr::memchr_iter(b'\n', &data[pos..end]) {
                    let next = pos + p + 1;
                    if next < len {
                        local.push(next as u64);
                    }
                }
                sh.starts.lock().unwrap().extend_from_slice(&local);
                sh.scanned.store(end as u64, Ordering::Release);
                pos = end;
            }
            sh.done.store(true, Ordering::Release);
        })
    }

    /// Incrementally index appended data (follow mode). Caller guarantees
    /// the previously scanned prefix is unchanged.
    pub fn extend(&self, data: &[u8]) {
        let from = self.sh.scanned.load(Ordering::Acquire) as usize;
        if from >= data.len() {
            return;
        }
        let mut v = self.sh.starts.lock().unwrap();
        if v.is_empty() {
            v.push(0);
        } else if from > 0 && data[from - 1] == b'\n' {
            // previous data ended exactly at a line boundary: the first new byte starts a line
            v.push(from as u64);
        }
        // if previous data ended without '\n', the last "line" continues; its
        // start is already recorded. New starts follow any new '\n'.
        scan_into(data, from, &mut v);
        self.sh.scanned.store(data.len() as u64, Ordering::Release);
        self.sh.done.store(true, Ordering::Release);
    }

    pub fn reset(&self) {
        self.sh.starts.lock().unwrap().clear();
        self.sh.scanned.store(0, Ordering::Release);
        self.sh.done.store(false, Ordering::Release);
    }

    pub fn count(&self) -> usize {
        self.sh.starts.lock().unwrap().len()
    }

    pub fn is_done(&self) -> bool {
        self.sh.done.load(Ordering::Acquire)
    }

    pub fn progress(&self, len: usize) -> f64 {
        if len == 0 { 1.0 } else { self.sh.scanned.load(Ordering::Acquire) as f64 / len as f64 }
    }

    /// Byte range of line `i` (without terminator), or None if not yet indexed.
    pub fn range(&self, i: usize, data: &[u8]) -> Option<(usize, usize)> {
        let v = self.sh.starts.lock().unwrap();
        let s = *v.get(i)? as usize;
        let e = match v.get(i + 1) {
            Some(&n) => n as usize,
            None => {
                if !self.is_done() {
                    return None;
                }
                data.len()
            }
        };
        let mut e = e.min(data.len());
        if e > s && data[e - 1] == b'\n' {
            e -= 1;
        }
        if e > s && data[e - 1] == b'\r' {
            e -= 1;
        }
        Some((s, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_basic() {
        assert_eq!(line_starts(b""), Vec::<u64>::new());
        assert_eq!(line_starts(b"a"), vec![0]);
        assert_eq!(line_starts(b"a\n"), vec![0]);
        assert_eq!(line_starts(b"a\nb"), vec![0, 2]);
        assert_eq!(line_starts(b"a\n\nb\n"), vec![0, 2, 3]);
    }

    #[test]
    fn range_strips_terminators() {
        let idx = LineIndex::new();
        let data = b"ab\r\ncd\nef";
        {
            let mut v = idx.sh.starts.lock().unwrap();
            *v = line_starts(data);
        }
        idx.sh.done.store(true, Ordering::Release);
        assert_eq!(idx.range(0, data), Some((0, 2)));
        assert_eq!(idx.range(1, data), Some((4, 6)));
        assert_eq!(idx.range(2, data), Some((7, 9)));
        assert_eq!(idx.range(3, data), None);
    }
}
