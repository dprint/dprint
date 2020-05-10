use super::super::parser_types::Context;

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

pub enum TextDecorationKind {
    Emphasis,
    Strong,
    Strikethrough,
}

pub struct TextDecoration {
    pub range: Range,
    pub kind: TextDecorationKind,
    pub children: Vec<Node>,
}

pub struct Html {
    pub range: Range,
    pub text: String,
}

pub struct FootnoteReference {
    pub range: Range,
    pub name: String,
}

pub struct FootnoteDefinition {
    pub range: Range,
    pub name: String,
    pub children: Vec<Node>,
}

pub struct InlineLink {
    pub range: Range,
    pub text: String,
    pub url: String,
    pub title: Option<String>,
}

pub struct ReferenceLink {
    pub range: Range,
    pub text: String,
    pub reference: String,
}

pub struct ShortcutLink {
    pub range: Range,
    pub text: String,
}

pub struct AutoLink {
    pub range: Range,
    pub text: String,
}

pub struct LinkReference {
    pub range: Range,
    pub name: String,
    pub link: String,
    pub title: Option<String>,
}

pub struct InlineImage {
    pub range: Range,
    pub text: String,
    pub url: String,
    pub title: Option<String>,
}

pub struct ReferenceImage {
    pub range: Range,
    pub text: String,
    pub reference: String,
}

impl Text {
    pub fn starts_with_punctuation(&self) -> bool {
        if let Some(first_char) = self.text.chars().next() {
            first_char.is_ascii_punctuation()
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

pub struct List {
    pub range: Range,
    pub start_index: Option<u64>,
    pub children: Vec<Node>,
}

pub struct Item {
    pub range: Range,
    pub children: Vec<Node>,
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
    Html,
    FootnoteReference,
    FootnoteDefinition,
    InlineLink,
    ReferenceLink,
    ShortcutLink,
    AutoLink,
    LinkReference,
    InlineImage,
    ReferenceImage,
    List,
    Item,
    SoftBreak,
    HardBreak,
    Code,
    CodeBlock
];
