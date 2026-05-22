use std::collections::VecDeque;
use parking_lot::Mutex;
use std::sync::Arc;

/// Rolling in-memory log buffer.
///
/// Stores up to `max_lines` most-recent lines. Older lines are dropped.
#[derive(Clone)]
pub struct LogBuffer {
    pub(crate) inner: Arc<Mutex<VecDeque<String>>>,
    pub(crate) max_lines: usize,
}

impl LogBuffer {
    pub fn new(max_lines: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(VecDeque::with_capacity(max_lines))),
            max_lines,
        }
    }

    pub fn push(&self, line: String) {
        let mut q = self.inner.lock();
        if q.len() == self.max_lines {
            q.pop_front();
        }
        q.push_back(line);
    }

    pub fn snapshot(&self) -> Vec<String> {
        self.inner.lock().iter().cloned().collect()
    }

    pub fn handle(&self) -> Arc<Mutex<VecDeque<String>>> {
        self.inner.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_appends_lines() {
        let buf = LogBuffer::new(3);
        buf.push("a".into());
        buf.push("b".into());
        assert_eq!(buf.snapshot(), vec!["a", "b"]);
    }

    #[test]
    fn push_drops_oldest_when_full() {
        let buf = LogBuffer::new(2);
        buf.push("a".into());
        buf.push("b".into());
        buf.push("c".into());
        assert_eq!(buf.snapshot(), vec!["b", "c"]);
    }

    #[test]
    fn snapshot_returns_independent_copy() {
        let buf = LogBuffer::new(5);
        buf.push("a".into());
        let snap = buf.snapshot();
        buf.push("b".into());
        assert_eq!(snap, vec!["a"]);
    }
}
