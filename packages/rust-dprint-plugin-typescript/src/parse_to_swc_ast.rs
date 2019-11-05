use std::sync::Arc;
use swc_common::{
    errors::{ColorConfig, Handler},
    FileName, comments::{Comments}, SourceFile, BytePos
};
use swc_ecma_ast::{Module};
use swc_ecma_parser::{Parser, Session, SourceFileInput, Syntax};

pub struct ParsedSourceFile {
    pub comments: Comments,
    pub module: Module,
    pub info: SourceFile,
}

pub fn parse_to_swc_ast(file_path: String, file_text: String) -> Result<ParsedSourceFile, String> {
    // todo: investigate if there's more of a lightweight way to do this or if this doesn't matter
    let handler = Handler::with_tty_emitter(ColorConfig::Auto, true, false, None);
    let session = Session { handler: &handler };

    let source_file = SourceFile::new(
        FileName::Custom(file_path.clone()),
        false,
        FileName::Custom(file_path),
        file_text,
        BytePos(0),
    );

    let comments: Comments = Default::default();
    let module = {
        let mut parser = Parser::new(
            session,
            Syntax::Typescript(Default::default()),
            SourceFileInput::from(&source_file),
            Some(&comments),
        );

        let parse_module_result = parser.parse_module();

        match parse_module_result {
            Err(error) => Err(error.message()),
            Ok(module) => Ok(module)
        }
    }?;

    Ok(ParsedSourceFile {
        comments,
        module,
        info: source_file
    })
}