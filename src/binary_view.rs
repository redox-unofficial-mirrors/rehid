pub struct BinaryView<'a> {
    data: &'a [u8],
    offset: usize,
    len: usize,
}
impl<'a> BinaryView<'a> {
    pub fn new(data: &'a [u8], offset: usize, len: usize) -> Self {
        Self { data, offset, len }
    }

    pub fn get(&self, bit_index: usize) -> Option<bool> {
        let bit_index = self.offset + bit_index;

        if bit_index >= self.offset + self.len {
            return None;
        }

        let byte_index = bit_index / 8;
        let bits_from_first = bit_index % 8;
        let byte = self.data.get(byte_index)?;
        Some(byte & (1 << bits_from_first) != 0)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn binary_view() {
        // 0000 1000 1100 0111
        //  E             S
        let view = super::BinaryView::new(&[0xC7, 0x08], 3, 11);
        assert_eq!(view.get(0), Some(false));
        assert_eq!(view.get(2), Some(false));
        assert_eq!(view.get(3), Some(true));
        assert_eq!(view.get(7), Some(false));
        assert_eq!(view.get(17), None);
    }
}
