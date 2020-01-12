use std::rc::Rc;
use super::*;
use swc_common::{
    errors::{ColorConfig, Handler},
    FileName, comments::{Comments}, SourceFile, BytePos
};
use swc_ecma_ast::{Module};
use swc_ecma_parser::{Parser, Session, SourceFileInput, Syntax, lexer::Lexer, Capturing, JscTarget};

pub struct ParsedSourceFile {
    pub comments: CommentCollection,
    pub token_finder: TokenFinder,
    pub module: Module,
    pub info: SourceFile,
    pub file_bytes: Rc<Vec<u8>>,
}

pub fn parse_to_swc_ast(file_path: &str, file_text: &str) -> Result<ParsedSourceFile, String> {
    let handler = Handler::with_tty_emitter(ColorConfig::Auto, true, false, None);
    let session = Session { handler: &handler };

    let file_bytes = Rc::new(file_text.as_bytes().to_vec());
    let source_file = SourceFile::new(
        FileName::Custom(file_path.into()),
        false,
        FileName::Custom(file_path.into()),
        file_text.into(),
        BytePos(0),
    );

    let comments: Comments = Default::default();
    let (module, tokens) = {
        let mut ts_config: swc_ecma_parser::TsConfig = Default::default();
        ts_config.tsx = should_parse_as_jsx(file_path);
        ts_config.dynamic_import = true;
        ts_config.decorators = true;
        let lexer = Lexer::new(
            session,
            Syntax::Typescript(ts_config),
            JscTarget::Es2019,
            SourceFileInput::from(&source_file),
            Some(&comments)
        );
        let lexer = Capturing::new(lexer);
        let mut parser = Parser::new_from(session, lexer);
        let parse_module_result = parser.parse_module();
        let tokens = Rc::new(parser.input().take());

        match parse_module_result {
            Err(error) => {
                println!("Error: {}", error.message());
                Err(error.message())
            },
            Ok(module) => Ok((module, tokens))
        }
    }?;

    let token_finder = TokenFinder::new(tokens.clone(), file_bytes.clone());
    return Ok(ParsedSourceFile {
        comments: CommentCollection::new(comments, TokenFinder::new(tokens, file_bytes.clone())),
        module,
        info: source_file,
        token_finder,
        file_bytes,
    });

    fn should_parse_as_jsx(file_path: &str) -> bool {
        if let Some(extension) = get_extension(&file_path) {
            return extension == "tsx" || extension == "jsx" || extension == "js";
        }
        return true;

        fn get_extension(file_path: &str) -> Option<String> {
            let period_pos = file_path.rfind('.')?;
            return Some(file_path[period_pos + 1..].to_lowercase());
        }
    }
}