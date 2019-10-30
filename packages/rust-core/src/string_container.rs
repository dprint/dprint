pub trait StringContainer : Clone {
    fn get_length(&self) -> usize;
    fn get_text(self) -> String;
}

impl StringContainer for String {
    fn get_length(&self) -> usize {
        self.chars().count()
    }

    fn get_text(self) -> String {
        self
    }
}
