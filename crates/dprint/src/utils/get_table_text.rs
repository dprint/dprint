pub fn get_table_text(items: Vec<(&str, &str)>, hanging_indent_size: usize) -> Vec<String> {
    let largest_name_len = {
        let mut key_lens = items.iter().map(|(key, _)| key.len()).collect::<Vec<_>>();
        key_lens.sort();
        key_lens.pop().unwrap_or(0)
    };

    items.iter().map(|(key, value)| {
        let mut text = String::new();
        text.push_str(key);
        for (i, line) in value.lines().enumerate() {
            if i == 0 { text.push_str(&" ".repeat(largest_name_len - key.len() + 1)); }
            else if i > 0 {
                text.push_str("\n");
                text.push_str(&" ".repeat(largest_name_len + hanging_indent_size + 1));
            }
            text.push_str(line);
        }
        text
    }).collect()
}
