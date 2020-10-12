pub fn get_terminal_width() -> Option<u16> {
    get_terminal_size().map(|(cols, _)| cols)
}

/// Gets the terminal size (width/cols, height/rows).
pub fn get_terminal_size() -> Option<(u16, u16)> {
    match crossterm::terminal::size() {
        Ok(size) => Some(size),
        Err(_) => None,
    }
}
