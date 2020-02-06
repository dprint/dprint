use super::*;
use std::collections::{HashMap};
use swc_common::{
    errors::{Handler, Emitter, DiagnosticBuilder},
    FileName, comments::{Comment, Comments, CommentMap}, SourceFile, BytePos
};
use swc_ecma_ast::{Module};
use swc_ecma_parser::{Parser, Session, SourceFileInput, Syntax, lexer::Lexer, Capturing, JscTarget, token::{TokenAndSpan}};

pub struct ParsedSourceFile {
    pub module: Module,
    pub info: SourceFile,
    pub file_bytes: Vec<u8>,
    pub tokens: Vec<TokenAndSpan>,
    pub leading_comments: HashMap<BytePos, Vec<Comment>>,
    pub trailing_comments: HashMap<BytePos, Vec<Comment>>,
}

pub fn parse_swc_ast(file_path: &str, file_text: &str) -> Result<ParsedSourceFile, String> {
    let handler = Handler::with_emitter(false, false, Box::new(EmptyEmitter {}));
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
            JscTarget::Es2019,
            SourceFileInput::from(&source_file),
            Some(&comments)
        );
        let lexer = Capturing::new(lexer);
        let mut parser = Parser::new_from(session, lexer);
        let parse_module_result = parser.parse_module();
        let tokens = parser.input().take();

        match parse_module_result {
            Err(mut error) => {
                // mark the diagnostic as being handled (otherwise it will panic in its drop)
                error.cancel();
                // return the formatted diagnostic string
                Err(format_diagnostic(&error, file_text))
            },
            Ok(module) => Ok((module, tokens))
        }
    }?;

    let (leading_comments, trailing_comments) = comments.take_all();

    return Ok(ParsedSourceFile {
        leading_comments: comment_map_to_hash_map(leading_comments),
        trailing_comments: comment_map_to_hash_map(trailing_comments),
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

    fn comment_map_to_hash_map(comments: CommentMap) -> HashMap<BytePos, Vec<Comment>> {
        // todo: This next comment needs updating because now it's a DashMap and
        // cloning is happening where previously it would take all the items out.

        // It is much more performant to look into HashMaps instead of CHashMaps
        // because then locking on each comment lookup is not necessary. We don't
        // need to support multi-threading so convert to HashMap.

        // todo: the cloning needs to be removed here!!
        return comments.iter_mut().map(|x| {
            let key = x.key().to_owned();
            let value = x.value().to_owned();
            (key, value)
        }).collect();
    }
}

pub struct EmptyEmitter {
}

impl Emitter for EmptyEmitter {
    fn emit(&mut self, _: &DiagnosticBuilder<'_>) {
        // for now, we don't care about diagnostics so do nothing
    }

    fn should_show_explain(&self) -> bool {
        false
    }
}

fn format_diagnostic(error: &DiagnosticBuilder, file_text: &str) -> String {
    // todo: handling sub diagnostics?
    let mut message = String::new();
    if let Some(primary_span) = error.span.primary_span() {
        let error_pos = primary_span.lo().0 as usize;
        let line_number = utils::get_line_number_of_pos(file_text, error_pos);
        let column_number = utils::get_column_number_of_pos(file_text, error_pos);
        message.push_str(&format!("Line {}, column {}: ", line_number, column_number))
    }
    message.push_str(&error.message());
    return message;
}
