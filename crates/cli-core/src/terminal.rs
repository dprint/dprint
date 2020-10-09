pub fn get_terminal_width() -> Option<u16> {
    match crossterm::terminal::size() {
        Ok((cols, _)) => Some(cols),
        Err(_) => None,
    }
}
