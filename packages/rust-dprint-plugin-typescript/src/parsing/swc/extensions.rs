use swc_ecma_ast::BinaryOp;

pub trait BinaryOpExtensions {
    fn is_add_sub(&self) -> bool;
    fn is_mul_div(&self) -> bool;
    fn is_bitwise_or_arithmetic(&self) -> bool;
    fn is_logical(&self) -> bool;
    fn is_bit_logical(&self) -> bool;
    fn is_bit_shift(&self) -> bool;
    fn is_equality(&self) -> bool;
}

impl BinaryOpExtensions for BinaryOp {
    fn is_add_sub(&self) -> bool {
        match self {
            BinaryOp::Add | BinaryOp::Sub => true,
            _ => false,
        }
    }

    fn is_mul_div(&self) -> bool {
        match self {
            BinaryOp::Mul | BinaryOp::Div => true,
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

    fn is_bit_logical(&self) -> bool {
        match self {
            BinaryOp::BitOr | BinaryOp::BitAnd | BinaryOp::BitXor => true,
            _ => false,
        }
    }

    fn is_bit_shift(&self) -> bool {
        match self {
            BinaryOp::LShift | BinaryOp::RShift | BinaryOp::ZeroFillRShift => true,
            _ => false,
        }
    }

    fn is_equality(&self) -> bool {
        match self {
            BinaryOp::EqEq | BinaryOp::NotEq | BinaryOp::EqEqEq | BinaryOp::NotEqEq | BinaryOp::Lt | BinaryOp::LtEq | BinaryOp::Gt | BinaryOp::GtEq => true,
            _ => false,
        }
    }
}
