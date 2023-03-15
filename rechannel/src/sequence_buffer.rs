use crate::packet::AckData;

pub(crate) trait Sequence: PartialEq + Eq + Default + Clone + Copy {
    fn greater_than(s1: Self, s2: Self) -> bool;
    fn less_than(s1: Self, s2: Self) -> bool;
    fn index(sequence: Self, entries: usize) -> usize;
    fn can_discard_sequence(&self, sequence: Self, entries_len: usize) -> bool;
    fn next(&self) -> Self;
}

impl Sequence for u16 {
    fn less_than(s1: Self, s2: Self) -> bool {
        Self::greater_than(s2, s1)
    }

    fn greater_than(s1: Self, s2: Self) -> bool {
        ((s1 > s2) && (s1 - s2 <= 32768)) || ((s1 < s2) && (s2 - s1 > 32768))
    }

    fn index(sequence: Self, entries: usize) -> usize {
        sequence as usize % entries
    }

    fn can_discard_sequence(&self, sequence: u16, entries_len: usize) -> bool {
        Self::less_than(sequence, self.wrapping_sub(entries_len as u16))
    }

    fn next(&self) -> Self {
        self.wrapping_add(1)
    }
}

impl Sequence for u64 {
    fn less_than(s1: Self, s2: Self) -> bool {
        s1 < s2
    }

    fn greater_than(s1: Self, s2: Self) -> bool {
        s1 > s2
    }

    fn index(sequence: Self, entries: usize) -> usize {
        sequence as usize % entries
    }

    fn can_discard_sequence(&self, sequence: u64, entries_len: usize) -> bool {
        sequence.wrapping_add(entries_len as u64) < *self
    }

    fn next(&self) -> Self {
        self.wrapping_add(1)
    }
}

#[derive(Debug)]
pub(crate) struct SequenceBuffer<T, N=u16> {
    next_sequence: N,
    entry_sequences: Box<[Option<N>]>,
    entries: Box<[Option<T>]>,
}

impl<T: Clone, N: Sequence> SequenceBuffer<T, N> {
    pub fn with_capacity(size: usize) -> Self {
        assert!(size > 0, "tried to initialize SequenceBuffer with 0 size");

        Self {
            next_sequence: N::default(),
            entry_sequences: vec![None; size].into_boxed_slice(),
            entries: vec![None; size].into_boxed_slice(),
        }
    }

    pub fn size(&self) -> usize {
        self.entries.len()
    }

    pub fn get_mut(&mut self, sequence: N) -> Option<&mut T> {
        if self.exists(sequence) {
            let index = self.index(sequence);
            return self.entries[index].as_mut();
        }
        None
    }

    #[allow(dead_code)]
    pub fn get(&self, sequence: N) -> Option<&T> {
        if self.exists(sequence) {
            let index = self.index(sequence);
            return self.entries[index].as_ref();
        }
        None
    }

    pub fn get_or_insert_with<F: FnOnce() -> T>(&mut self, sequence: N, f: F) -> Option<&mut T> {
        if self.exists(sequence) {
            let index = self.index(sequence);
            self.entries[index].as_mut()
        } else {
            self.insert(sequence, f())
        }
    }

    #[inline]
    pub fn index(&self, sequence: N) -> usize {
        N::index(sequence, self.entries.len())
    }

    pub fn available(&self, sequence: N) -> bool {
        let index = self.index(sequence);
        self.entry_sequences[index].is_none()
    }

    /// Returns whether or not we have previously inserted an entry for the given sequence number.
    pub fn exists(&self, sequence: N) -> bool {
        let index = self.index(sequence);
        if let Some(s) = self.entry_sequences[index] {
            return s == sequence;
        }
        false
    }

    pub fn insert(&mut self, sequence: N, data: T) -> Option<&mut T> {
        if self.next_sequence.can_discard_sequence(sequence, self.entry_sequences.len()) {
            return None;
        }

        // If the new element has a greater sequence
        // Remove old sequences that are between the current sequence and the new one
        if N::greater_than(sequence.next(), self.next_sequence) {
            let mut next = self.next_sequence;
            while next != sequence {
                self.remove(next);
                next = next.next();
            }
            self.next_sequence = sequence.next();
        }

        let index = self.index(sequence);
        self.entry_sequences[index] = Some(sequence);
        self.entries[index] = Some(data);
        self.entries[index].as_mut()
    }

    pub fn remove(&mut self, sequence: N) -> Option<T> {
        if self.exists(sequence) {
            let index = self.index(sequence);
            self.entry_sequences[index] = None;
            let value = self.entries[index].take();
            return value;
        }
        None
    }

    #[inline]
    pub fn sequence(&self) -> N {
        self.next_sequence
    }
}

// Since sequences can wrap we need to check when this when checking greater
// Ocurring the cutover in the middle of u16
#[inline]
pub fn sequence_greater_than(s1: u16, s2: u16) -> bool {
    ((s1 > s2) && (s1 - s2 <= 32768)) || ((s1 < s2) && (s2 - s1 > 32768))
}

#[inline]
pub fn sequence_less_than(s1: u16, s2: u16) -> bool {
    sequence_greater_than(s2, s1)
}

#[cfg(test)]
mod tests {
    use super::SequenceBuffer;

    #[derive(Clone, Default)]
    struct DataStub;

    #[test]
    fn max_sequence_not_exists_by_default() {
        let buffer: SequenceBuffer<DataStub, u64> = SequenceBuffer::with_capacity(8);
        assert!(!buffer.exists(u64::max_value()));
    }

    #[test]
    fn insert() {
        let mut buffer: SequenceBuffer<DataStub, u64> = SequenceBuffer::with_capacity(2);
        buffer.insert(0, DataStub).unwrap();
        assert!(buffer.exists(0));
    }

    #[test]
    fn remove() {
        let mut buffer: SequenceBuffer<DataStub, u64> = SequenceBuffer::with_capacity(2);
        buffer.insert(0, DataStub).unwrap();
        let removed = buffer.remove(0);
        assert!(removed.is_some());
        assert!(!buffer.exists(0));
    }

    fn count_entries(buffer: &SequenceBuffer<DataStub, u64>) -> usize {
        buffer.entry_sequences.iter().flatten().count()
    }

    #[test]
    fn insert_over_older_entries() {
        let mut buffer: SequenceBuffer<DataStub, u64> = SequenceBuffer::with_capacity(8);
        buffer.insert(8, DataStub).unwrap();
        buffer.insert(0, DataStub);
        assert!(!buffer.exists(0));

        buffer.insert(16, DataStub);
        assert!(buffer.exists(16));

        assert_eq!(count_entries(&buffer), 1);
    }

    #[test]
    fn insert_old_entries() {
        let mut buffer: SequenceBuffer<DataStub, u64> = SequenceBuffer::with_capacity(8);
        buffer.insert(11, DataStub);
        buffer.insert(2, DataStub);
        assert!(!buffer.exists(2));

        buffer.insert(u64::max_value(), DataStub);
        assert!(!buffer.exists(u64::max_value()));

        assert_eq!(count_entries(&buffer), 1);
    }

    // #[test]
    // fn ack_bits() {
    //     let mut buffer: SequenceBuffer<DataStub, u64> = SequenceBuffer::with_capacity(64);
    //     buffer.insert(0, DataStub).unwrap();
    //     buffer.insert(1, DataStub).unwrap();
    //     buffer.insert(3, DataStub).unwrap();
    //     buffer.insert(4, DataStub).unwrap();
    //     buffer.insert(5, DataStub).unwrap();
    //     buffer.insert(7, DataStub).unwrap();
    //     buffer.insert(30, DataStub).unwrap();
    //     buffer.insert(31, DataStub).unwrap();
    //     let ack_data = buffer.ack_data(buffer.sequence().wrapping_sub(1));

    //     assert_eq!(ack_data.ack, 31);
    //     assert_eq!(ack_data.ack_bits, 0b11011101000000000000000000000011u32);
    // }

    #[test]
    fn available() {
        let mut buffer: SequenceBuffer<DataStub, u64> = SequenceBuffer::with_capacity(2);
        buffer.insert(0, DataStub).unwrap();
        buffer.insert(1, DataStub).unwrap();
        assert!(!buffer.available(2));
    }
}
