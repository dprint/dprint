use swc_ecma_ast::*;
use swc_ecma_parser::{token::{TokenAndSpan}};

use super::*;
use super::super::*;
use super::super::super::configuration::*;

pub struct BinExprItem<'a> {
    pub pre_op: Option<BinExprOp<'a>>,
    pub post_op: Option<BinExprOp<'a>>,
    pub expr: Node<'a>,
}

#[derive(Clone)]
pub struct BinExprOp<'a> {
    pub token: &'a TokenAndSpan,
    pub op: BinaryOp,
}

pub fn get_flattened_bin_expr<'a>(node: &'a BinExpr, context: &mut Context<'a>) -> Vec<BinExprItem<'a>> {
    let mut items = Vec::new();
    let operator_token = BinExprOp {
        token: context.token_finder.get_first_operator_after(&*node.left, node.op.as_str()).unwrap(),
        op: node.op,
    };
    let is_op_same_line = get_operator_position(&node, &operator_token.token, context) == OperatorPosition::SameLine;
    let mut handled_left = false;
    let mut handled_right = false;

    if let Expr::Bin(left_bin) = &*node.left {
        if is_expression_breakable(&node.op, &left_bin.op) {
            items.extend(get_flattened_bin_expr(left_bin, context));
            if is_op_same_line {
                items.last_mut().unwrap().post_op = Some(operator_token.clone());
            }
            handled_left = true;
        }
    }

    if !handled_left {
        items.push(BinExprItem {
            pre_op: None,
            post_op: if is_op_same_line { Some(operator_token.clone()) } else { None },
            expr: (&*node.left).into(),
        });
    }

    if let Expr::Bin(right_bin) = &*node.right {
        if is_expression_breakable(&node.op, &right_bin.op) {
            let mut right_items = get_flattened_bin_expr(right_bin, context);
            if !is_op_same_line {
                right_items.first_mut().unwrap().pre_op = Some(operator_token.clone());
            }
            items.extend(right_items);
            handled_right = true;
        }
    }

    if !handled_right {
        items.push(BinExprItem {
            pre_op: if !is_op_same_line { Some(operator_token) } else { None },
            post_op: None,
            expr: (&*node.right).into(),
        });
    }

    return items;

    fn is_expression_breakable(top_op: &BinaryOp, op: &BinaryOp) -> bool {
        if top_op.is_add_sub() {
            op.is_add_sub()
        } else if top_op.is_mul_div() {
            op.is_mul_div()
        } else {
            top_op == op
        }
    }

    fn get_operator_position(node: &BinExpr, operator_token: &TokenAndSpan, context: &mut Context) -> OperatorPosition {
        match context.config.binary_expression_operator_position {
            OperatorPosition::NextLine => OperatorPosition::NextLine,
            OperatorPosition::SameLine => OperatorPosition::SameLine,
            OperatorPosition::Maintain => {
                if node.left.end_line(context) == operator_token.start_line(context) {
                    OperatorPosition::SameLine
                } else {
                    OperatorPosition::NextLine
                }
            }
        }
    }
}
