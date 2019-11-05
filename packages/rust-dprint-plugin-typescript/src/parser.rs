extern crate dprint_core;

use dprint_core::*;
use super::*;
use swc_ecma_ast::{ModuleItem, Stmt, Expr, Lit};

pub fn parse(source_file: ParsedSourceFile) -> Vec<PrintItem> {
    for item in source_file.module.body {
        match item {
            ModuleItem::Stmt(stmt) => {
                match stmt {
                    Stmt::Expr(expr) => {
                        match *expr {
                            Expr::Lit(lit) => {
                                match lit {
                                    Lit::Str(text) => {
                                        println!("{}", text.value);
                                    },
                                    _ => {}
                                }
                            },
                            _ => {}
                        }
                    },
                    _ => {},
                }
            },
            _ => {},
        };
    }
    let items = Vec::new();
    items
}
