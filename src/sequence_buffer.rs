pub struct SequenceBuffer<T: Clone + Default> {
    sequence: u16,
    entry_sequences: Box<[Option<u16>]>,
    entries: Box<[T]>,
}

impl<T: Clone + Default> SequenceBuffer<T> {
    pub fn with_capacity(size: usize) -> Self {
        Self {
            sequence: 0,
            entry_sequences: vec![None; size as usize].into_boxed_slice(),
            entries: vec![T::default(); size as usize].into_boxed_slice(),
        }
    }

    pub fn get_mut(&mut self, sequence: u16) -> Option<&mut T> {
        if self.exists(sequence) {
            let index = self.index(sequence);
            return Some(&mut self.entries[index]);
        }
        None
    }
    
    #[inline]
    pub fn index(&self, sequence: u16) -> usize {
        sequence as usize % self.entries.len()
    }

    /// Returns whether or not we have previously inserted an entry for the given sequence number.
    pub fn exists(&self, sequence: u16) -> bool {
        let index = self.index(sequence);
        if let Some(s) = self.entry_sequences[index] {
            return s == sequence;
        }
        false
    }

    pub fn insert(&mut self, sequence: u16, data: T) -> Option<&mut T> {
        if sequence_less_than(
            sequence,
            self.sequence
                .wrapping_sub(self.entry_sequences.len() as u16),
        ) {
            return None;
        }
        
        // TODO: investigate why adding 1 here, we subtracting 1 when generating ack because of this
        if sequence_greater_than(sequence.wrapping_add(1), self.sequence) {
            self.remove_entries(u32::from(sequence));
            self.sequence = sequence.wrapping_add(1);
        }

        let index = self.index(sequence);
        self.entry_sequences[index] = Some(sequence);
        self.entries[index] = data;
        Some(&mut self.entries[index])
    }

    fn remove_entries(&mut self, mut finish_sequence: u32) {
        let start_sequence = u32::from(self.sequence);
        if finish_sequence < start_sequence {
            finish_sequence += 65536;
        }

        if finish_sequence - start_sequence < self.entry_sequences.len() as u32 {
            for sequence in start_sequence..=finish_sequence {
                self.remove(sequence as u16);
            }
        } else {
            for index in 0..self.entry_sequences.len() {
                self.entries[index] = T::default();
                self.entry_sequences[index] = None;
            }
        }
    }

    pub fn remove(&mut self, sequence: u16) -> Option<T> {
        if self.exists(sequence) {
            let index = self.index(sequence);
            let value = std::mem::replace(&mut self.entries[index], T::default());
            self.entry_sequences[index] = None;
            return Some(value);
        }
        None
    }

    #[inline]
    pub fn len(&mut self) -> usize {
        self.entries.len()
    }
    
    #[inline]
    pub fn sequence(&self) -> u16 {
        self.sequence
    }

    pub fn ack_bits(&self) -> (u16, u32) {
        let ack = self.sequence().wrapping_sub(1);
        let mut ack_bits = 0;

        for i in 1..=32 {
            let sequence = ack.wrapping_sub(i);
            if self.exists(sequence) {
                ack_bits |= 1;
            }
            ack_bits <<= 1;
        }

        (ack, ack_bits)
    }
}

// Since sequences can wrap we need to check when this when checking greater
// Ocurring the cutover in the middle of u16
#[inline]
fn sequence_greater_than(s1: u16, s2: u16) -> bool {
    ((s1 > s2) && (s1 - s2 <= 32768)) || ((s1 < s2) && (s2 - s1 > 32768))
}

#[inline]
fn sequence_less_than(s1: u16, s2: u16) -> bool {
    sequence_greater_than(s2, s1)
}

#[cfg(test)]
mod tests {
    use super::SequenceBuffer;

    #[derive(Clone, Default)]
    struct DataStub;

    #[test]
    fn max_sequence_not_exists_by_default() {
        let buffer: SequenceBuffer<DataStub> = SequenceBuffer::with_capacity(8);
        assert!(!buffer.exists(u16::max_value()));
    }

    #[test]
    fn insert() {
        let mut buffer = SequenceBuffer::with_capacity(2);
        buffer.insert(0, DataStub).unwrap();
        assert!(buffer.exists(0));
    }

    #[test]
    fn remove() {
        let mut buffer = SequenceBuffer::with_capacity(2);
        buffer.insert(0, DataStub).unwrap();
        buffer.remove(0);
        assert!(!buffer.exists(0));
    }

    #[test]
    fn insert_over_older_entries() {
        let mut buffer = SequenceBuffer::with_capacity(8);
        buffer.insert(8, DataStub).unwrap();
        buffer.insert(0, DataStub);
        assert!(!buffer.exists(0));

        buffer.insert(16, DataStub);
        assert!(buffer.exists(16));

        assert_eq!(count_entries(&buffer), 1);
    }

    #[test]
    fn insert_old_entries() {
        let mut buffer = SequenceBuffer::with_capacity(8);
        buffer.insert(11, DataStub);
        buffer.insert(2, DataStub);
        assert!(!buffer.exists(2));

        buffer.insert(u16::max_value(), DataStub);
        assert!(!buffer.exists(u16::max_value()));

        assert_eq!(count_entries(&buffer), 1);
    }

    fn count_entries(buffer: &SequenceBuffer<DataStub>) -> usize {
        let nums: Vec<&u16> = buffer.entry_sequences.iter().flatten().collect();
        nums.len()
    }
}
