pub enum Batch<'a> {
    Fixed(&'a [u8], usize),
    Variable(&'a [u8], &'a [u32]),
}
