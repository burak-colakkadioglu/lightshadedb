use crate::table::FieldType;
use std::ops::{BitAnd, BitOr, Not, RangeInclusive};

pub struct Field<T, V> {
    name: &'static str,
    ty: FieldType,
    table: std::marker::PhantomData<fn() -> (T, V)>,
}

impl<T, V> Field<T, V> {
    pub const fn new(name: &'static str, ty: FieldType) -> Self {
        Self {
            name,
            ty,
            table: std::marker::PhantomData,
        }
    }

    pub const fn name(self) -> &'static str {
        self.name
    }

    pub const fn field_type(self) -> FieldType {
        self.ty
    }

    pub fn eq(self, value: impl Into<V>) -> Predicate {
        let _ = value.into();
        Predicate::new()
    }

    pub fn ne(self, value: impl Into<V>) -> Predicate {
        let _ = value.into();
        Predicate::new()
    }

    pub fn lt(self, value: impl Into<V>) -> Predicate {
        let _ = value.into();
        Predicate::new()
    }

    pub fn le(self, value: impl Into<V>) -> Predicate {
        let _ = value.into();
        Predicate::new()
    }

    pub fn gt(self, value: impl Into<V>) -> Predicate {
        let _ = value.into();
        Predicate::new()
    }

    pub fn ge(self, value: impl Into<V>) -> Predicate {
        let _ = value.into();
        Predicate::new()
    }

    pub fn eq_field<U>(self, other: Field<U, V>) -> Predicate {
        let _ = other;
        Predicate::new()
    }

    pub fn ne_field<U>(self, other: Field<U, V>) -> Predicate {
        let _ = other;
        Predicate::new()
    }

    pub fn between(self, range: RangeInclusive<V>) -> Predicate {
        let _ = range;
        Predicate::new()
    }

    pub fn in_(self, values: impl IntoIterator<Item = V>) -> Predicate {
        for value in values {
            let _ = value;
        }
        Predicate::new()
    }

    pub fn asc(self) -> Order {
        Order::new()
    }

    pub fn desc(self) -> Order {
        Order::new()
    }

    pub fn set(self, value: impl Into<V>) -> Assignment {
        let _ = value.into();
        Assignment::new()
    }

    pub fn group(self) -> Group {
        Group::new()
    }

    pub fn select(self) -> SelectValue<V> {
        SelectValue::new()
    }
}

impl<T, V> Field<T, Option<V>> {
    pub fn is_null(self) -> Predicate {
        Predicate::new()
    }

    pub fn is_not_null(self) -> Predicate {
        Predicate::new()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Predicate {
    _private: (),
}

impl Predicate {
    pub const fn new() -> Self {
        Self { _private: () }
    }

    pub fn and(self, other: Predicate) -> Predicate {
        let _ = other;
        Self::new()
    }

    pub fn or(self, other: Predicate) -> Predicate {
        let _ = other;
        Self::new()
    }

    #[allow(clippy::should_implement_trait)]
    pub fn not(self) -> Predicate {
        Self::new()
    }
}

impl Default for Predicate {
    fn default() -> Self {
        Self::new()
    }
}

impl BitAnd for Predicate {
    type Output = Predicate;

    fn bitand(self, rhs: Self) -> Self::Output {
        self.and(rhs)
    }
}

impl BitOr for Predicate {
    type Output = Predicate;

    fn bitor(self, rhs: Self) -> Self::Output {
        self.or(rhs)
    }
}

impl Not for Predicate {
    type Output = Predicate;

    fn not(self) -> Self::Output {
        Predicate::new()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Order {
    _private: (),
}

impl Order {
    pub const fn new() -> Self {
        Self { _private: () }
    }
}

impl Default for Order {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Assignment {
    _private: (),
}

impl Assignment {
    pub const fn new() -> Self {
        Self { _private: () }
    }
}

impl Default for Assignment {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Group {
    _private: (),
}

impl Group {
    pub const fn new() -> Self {
        Self { _private: () }
    }
}

impl Default for Group {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Join {
    _private: (),
}

impl Join {
    pub const fn new() -> Self {
        Self { _private: () }
    }
}

impl Default for Join {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SelectValue<V> {
    value: std::marker::PhantomData<fn() -> V>,
    _private: (),
}

impl<V> SelectValue<V> {
    pub const fn new() -> Self {
        Self {
            value: std::marker::PhantomData,
            _private: (),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Aggregate<V> {
    value: std::marker::PhantomData<fn() -> V>,
    _private: (),
}

impl<V> Aggregate<V> {
    pub const fn new() -> Self {
        Self {
            value: std::marker::PhantomData,
            _private: (),
        }
    }
}

impl<V> Default for SelectValue<V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<V> Default for Aggregate<V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<V> From<Aggregate<V>> for SelectValue<V> {
    fn from(_: Aggregate<V>) -> Self {
        Self::new()
    }
}

impl<V> From<Aggregate<Option<V>>> for SelectValue<V> {
    fn from(_: Aggregate<Option<V>>) -> Self {
        Self::new()
    }
}

pub fn count() -> Aggregate<i64> {
    Aggregate::new()
}

pub fn min<T, V>(field: Field<T, V>) -> Aggregate<V> {
    let _ = field;
    Aggregate::new()
}

pub fn max<T, V>(field: Field<T, V>) -> Aggregate<V> {
    let _ = field;
    Aggregate::new()
}

pub fn sum<T, V>(field: Field<T, V>) -> Aggregate<V> {
    let _ = field;
    Aggregate::new()
}

pub fn avg<T, V>(field: Field<T, V>) -> Aggregate<V> {
    let _ = field;
    Aggregate::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Note;
    const SCORE: Field<Note, i64> = Field::new("score", FieldType::I64);
    const RATIO: Field<Note, f64> = Field::new("ratio", FieldType::F64);
    const TITLE: Field<Note, String> = Field::new("title", FieldType::Text);

    #[test]
    fn predicates_combine() {
        let predicate = SCORE.gt(1) & TITLE.eq("a") | !TITLE.eq("b");
        let _ = predicate.and(SCORE.lt(9)).or(SCORE.eq(5)).not();
    }

    #[test]
    fn aggregates_convert_to_selected_values() {
        let _: SelectValue<i64> = count().into();
        let _: SelectValue<i64> = max(SCORE).into();
        let _: SelectValue<i64> = avg(SCORE).into();
        let _: SelectValue<f64> = avg(RATIO).into();
    }

    #[test]
    fn field_metadata_is_available() {
        assert_eq!(SCORE.name(), "score");
        assert_eq!(TITLE.field_type(), FieldType::Text);
    }
}
