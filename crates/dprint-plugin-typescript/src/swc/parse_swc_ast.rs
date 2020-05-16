use std::collections::{HashMap};
use std::path::PathBuf;
use swc_common::{
    errors::{Handler, Emitter, DiagnosticBuilder},
    FileName, comments::{Comment, Comments, CommentMap}, SourceFile, BytePos
};
use swc_ecma_ast::{Module};
use swc_ecma_parser::{Parser, Session, SourceFileInput, Syntax, lexer::Lexer, Capturing, JscTarget, token::{TokenAndSpan}};

pub struct ParsedSourceFile<'a> {
    pub module: Module,
    pub info: SourceFile,
    pub file_bytes: &'a [u8],
    pub tokens: Vec<TokenAndSpan>,
    pub leading_comments: HashMap<BytePos, Vec<Comment>>,
    pub trailing_comments: HashMap<BytePos, Vec<Comment>>,
}

pub fn parse_swc_ast<'a>(file_path: &PathBuf, file_text: &'a str) -> Result<ParsedSourceFile<'a>, String> {
    match parse_inner(file_path, file_text) {
        Ok(result) => Ok(result),
        Err(err) => {
            if get_lowercase_extension(file_path) == Some(String::from("ts")) {
                // try to parse as jsx
                let tsx_file_path = file_path.with_extension("tsx");
                match parse_inner(&tsx_file_path, file_text) {
                    Ok(result) => Ok(result),
                    Err(_) => Err(err), // return the original error
                }
            } else {
                Err(err)
            }
        }
    }
}

fn parse_inner<'a>(file_path: &PathBuf, file_text: &'a str) -> Result<ParsedSourceFile<'a>, String> {
    let handler = Handler::with_emitter(false, false, Box::new(EmptyEmitter {}));
    let session = Session { handler: &handler };

    let file_bytes = file_text.as_bytes();
    let source_file = SourceFile::new(
        FileName::Custom(file_path.to_string_lossy().into()),
        false,
        FileName::Custom(file_path.to_string_lossy().into()),
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

    fn should_parse_as_jsx(file_path: &PathBuf) -> bool {
        if let Some(extension) = get_lowercase_extension(file_path) {
            return extension == "tsx" || extension == "jsx" || extension == "js";
        }
        return true;
    }

    fn comment_map_to_hash_map(comments: CommentMap) -> HashMap<BytePos, Vec<Comment>> {
        // todo: This next comment needs updating because now it's a DashMap and
        // cloning is happening where previously it would take all the items out.

        // It is much more performant to look into HashMaps instead of CHashMaps
        // because then locking on each comment lookup is not necessary. We don't
        // need to support multi-threading so convert to HashMap.
        comments.into_iter().collect()
    }
}

fn get_lowercase_extension(file_path: &PathBuf) -> Option<String> {
    file_path.extension().and_then(|e| e.to_str()).map(|f| f.to_lowercase())
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
    dprint_core::utils::string_utils::format_diagnostic(
        error.span.primary_span().map(|span| (span.lo().0 as usize, span.hi().0 as usize)),
        &error.message(),
        file_text
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_error_on_syntax_diagnostic() {
        let message = parse_swc_ast(&PathBuf::from("./test.ts"), "test;\nas#;").err().unwrap();
        assert_eq!(
            message,
            concat!(
                "Line 2, column 3: Expected ';', '}' or <eof>\n",
                "\n",
                "  as#;\n",
                "    ~"
            )
        );
    }
}
