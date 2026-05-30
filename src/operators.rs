//! ClickHouse-specific infix operators.

use diesel::expression::Expression;

diesel::infix_operator!(GlobalIn, " GLOBAL IN ");
diesel::infix_operator!(NotGlobalIn, " GLOBAL NOT IN ");

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
