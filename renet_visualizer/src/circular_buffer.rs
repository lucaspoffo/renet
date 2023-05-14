#[derive(Debug)]
pub struct CircularBuffer<const N: usize, T> {
    pub(crate) queue: [T; N],
    cursor: usize,
}

impl<const N: usize, T: Default + Copy> Default for CircularBuffer<N, T> {
    fn default() -> Self {
        Self {
            queue: [T::default(); N],
            cursor: 0,
        }
    }
}

impl<const N: usize, T: Default + Copy> CircularBuffer<N, T> {
    pub fn push(&mut self, value: T) {
        self.queue[self.cursor] = value;
        self.cursor = (self.cursor + 1) % N;
    }

    pub fn as_vec(&self) -> Vec<T> {
        let (end, start) = self.queue.split_at(self.cursor);
        let mut vec = Vec::with_capacity(N);
        vec.extend_from_slice(start);
        vec.extend_from_slice(end);

        vec
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage() {
        let mut buffer: CircularBuffer<3, usize> = CircularBuffer::default();
        assert_eq!(buffer.as_vec(), vec![0, 0, 0]);

        buffer.push(1);
        buffer.push(2);
        buffer.push(3);
        assert_eq!(buffer.as_vec(), vec![1, 2, 3]);

        buffer.push(4);
        buffer.push(5);
        assert_eq!(buffer.as_vec(), vec![3, 4, 5]);
    }
}
