use swc_ecma_ast::BinaryOp;

pub trait BinaryOpExtensions {
    fn is_add_sub_mul_div(&self) -> bool;
    fn is_bitwise_or_arithmetic(&self) -> bool;
    fn is_logical(&self) -> bool;
}

impl BinaryOpExtensions for BinaryOp {
    fn is_add_sub_mul_div(&self) -> bool {
        match self {
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div => true,
            _ => false,
        }
    }

    fn is_bitwise_or_arithmetic(&self) -> bool {
        match self {
            BinaryOp::LShift | BinaryOp::RShift | BinaryOp::ZeroFillRShift | BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul
                | BinaryOp::Div | BinaryOp::Mod | BinaryOp::BitOr | BinaryOp::BitXor
                | BinaryOp::BitAnd => true,
            _ => false,
        }
    }

    fn is_logical(&self) -> bool {
        match self {
            BinaryOp::LogicalAnd | BinaryOp::LogicalOr => true,
            _ => false,
        }
    }
}
