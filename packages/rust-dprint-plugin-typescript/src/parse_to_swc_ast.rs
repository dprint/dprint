use swc_common::{
    errors::{ColorConfig, Handler},
    FileName, comments::{Comments}, SourceFile, BytePos
};
use swc_ecma_ast::{Module};
use swc_ecma_parser::{Parser, Session, SourceFileInput, Syntax, lexer::Lexer, Capturing, token::{Token, TokenAndSpan}};

pub struct ParsedSourceFile {
    pub comments: Comments,
    pub tokens: Vec<TokenAndSpan>,
    pub module: Module,
    pub info: SourceFile,
    pub file_bytes: Vec<u8>,
}

pub fn parse_to_swc_ast(file_path: &str, file_text: &str) -> Result<ParsedSourceFile, String> {
    // todo: investigate if there's more of a lightweight way to do this or if this doesn't matter
    let handler = Handler::with_tty_emitter(ColorConfig::Auto, true, false, None);
    let session = Session { handler: &handler };

    let file_bytes = file_text.as_bytes().to_vec();
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
            Default::default(),
            SourceFileInput::from(&source_file),
            Some(&comments)
        );
        let capturing = Capturing::new(lexer);
        let mut parser = Parser::new_from(session, capturing);
        let parse_module_result = parser.parse_module();

        let tokens = parser.input().take();

        match parse_module_result {
            Err(error) => {
                println!("Error: {}", error.message());
                Err(error.message())
            },
            Ok(module) => Ok((module, tokens))
        }
    }?;

    return Ok(ParsedSourceFile {
        comments,
        module,
        info: source_file,
        tokens,
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