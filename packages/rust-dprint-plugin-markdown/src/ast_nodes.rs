use super::parser_types::Context;

pub type Range = std::ops::Range<usize>;

pub trait Ranged {
    fn range<'a>(&'a self) -> &'a Range;
    fn text<'a>(&self, context: &Context<'a>) -> &'a str;
}

pub struct SourceFile {
    pub range: Range,
    pub children: Vec<Node>,
}

pub struct Heading {
    pub range: Range,
    pub level: u32,
    pub children: Vec<Node>,
}

pub struct Paragraph {
    pub range: Range,
    pub children: Vec<Node>,
}

pub struct BlockQuote {
    pub range: Range,
    pub children: Vec<Node>,
}

pub struct Text {
    pub range: Range,
    pub text: String,
}

pub struct TextDecoration {
    pub range: Range,
    pub kind: TextDecorationKind,
    pub children: Vec<Node>,
}

pub enum TextDecorationKind {
    Emphasis,
    Strong,
    Strikethrough,
}

pub struct Link {
    pub range: Range,
    pub link_type: pulldown_cmark::LinkType,
    pub reference: String,
    pub title: Option<String>,
    pub children: Vec<Node>,
}

impl Text {
    pub fn starts_with_whitespace(&self) -> bool {
        if let Some(first_char) = self.text.chars().next() {
            first_char.is_whitespace()
        } else {
            false
        }
    }

    pub fn starts_with_punctuation(&self) -> bool {
        if let Some(first_char) = self.text.chars().next() {
            first_char.is_ascii_punctuation()
        } else {
            false
        }
    }

    pub fn ends_with_whitespace(&self) -> bool {
        if let Some(last_char) = self.text.chars().last() {
            last_char.is_whitespace()
        } else {
            false
        }
    }
}

pub struct SoftBreak {
    pub range: Range,
}

pub struct HardBreak {
    pub range: Range,
}

/// Inline code.
pub struct Code {
    pub range: Range,
    pub code: String,
}

pub struct CodeBlock {
    pub range: Range,
    pub tag: Option<String>,
    pub code: String,
}

pub struct NotImplemented {
    pub range: Range,
}

macro_rules! generate_node {
    ($($node_name:ident),*) => {
        pub enum Node {
            $($node_name($node_name)),*,
        }

        impl Ranged for Node {
            fn range<'a>(&'a self) -> &'a Range {
                match self {
                    $(Node::$node_name(node) => node.range()),*
                }
            }

            fn text<'a>(&self, context: &Context<'a>) -> &'a str {
                match self {
                    $(Node::$node_name(node) => node.text(context)),*
                }
            }
        }

        $(
        impl Ranged for $node_name {
            fn range<'a>(&'a self) -> &'a Range {
                &self.range
            }

            fn text<'a>(&self, context: &Context<'a>) -> &'a str {
                &context.file_text[self.range.start..self.range.end]
            }
        }

        impl Into<Node> for $node_name {
            fn into(self) -> Node {
                Node::$node_name(self)
            }
        }
        )*
    };
}

generate_node![
    NotImplemented,
    SourceFile,
    Heading,
    Paragraph,
    BlockQuote,
    Text,
    TextDecoration,
    Link,
    SoftBreak,
    HardBreak,
    Code,
    CodeBlock
];
