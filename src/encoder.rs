trait Encoder<R: Read> {
    fn init(&mut self, rdr: R);
    fn find_match();
    fn update(&mut self, n_bytes: usize);
}