//! ClickHouse-specific infix operators.

use diesel::expression::{AsExpression, Expression};
use diesel::sql_types::SqlType;

diesel::infix_operator!(GlobalIn, " GLOBAL IN ");
diesel::infix_operator!(NotGlobalIn, " GLOBAL NOT IN ");
diesel::infix_operator!(ILike, " ILIKE ");
diesel::infix_operator!(NotILike, " NOT ILIKE ");

/// ClickHouse-specific methods for text predicates that Diesel does not expose generically.
pub trait ClickHouseTextExpressionMethods: Expression + Sized {
    /// Build `left ILIKE pattern`.
    fn ilike<T>(self, pattern: T) -> ILike<Self, T::Expression>
    where
        Self::SqlType: SqlType,
        T: AsExpression<Self::SqlType>,
    {
        ILike::new(self, pattern.as_expression())
    }

    /// Build `left NOT ILIKE pattern`.
    fn not_ilike<T>(self, pattern: T) -> NotILike<Self, T::Expression>
    where
        Self::SqlType: SqlType,
        T: AsExpression<Self::SqlType>,
    {
        NotILike::new(self, pattern.as_expression())
    }
}

impl<T> ClickHouseTextExpressionMethods for T where T: Expression {}

/// Fluent `.global_in(rhs)` for ClickHouse distributed subquery membership.
pub trait GlobalInDsl: Sized {
    fn global_in<Rhs>(self, rhs: Rhs) -> GlobalIn<Self, Rhs>
    where
        Rhs: Expression;
}

impl<T> GlobalInDsl for T
where
    T: Expression,
{
    fn global_in<Rhs>(self, rhs: Rhs) -> GlobalIn<Self, Rhs>
    where
        Rhs: Expression,
    {
        GlobalIn::new(self, rhs)
    }
}

/// Fluent `.not_global_in(rhs)` for `GLOBAL NOT IN`.
pub trait NotGlobalInDsl: Sized {
    fn not_global_in<Rhs>(self, rhs: Rhs) -> NotGlobalIn<Self, Rhs>
    where
        Rhs: Expression;
}

impl<T> NotGlobalInDsl for T
where
    T: Expression,
{
    fn not_global_in<Rhs>(self, rhs: Rhs) -> NotGlobalIn<Self, Rhs>
    where
        Rhs: Expression,
    {
        NotGlobalIn::new(self, rhs)
    }
}
