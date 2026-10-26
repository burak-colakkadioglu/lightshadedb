use crate::error::Result;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FieldType {
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    F32,
    F64,
    Bool,
    Text,
    Bytes,
    Uuid,
}

impl FieldType {
    pub fn is_signed_integer(self) -> bool {
        matches!(self, Self::I8 | Self::I16 | Self::I32 | Self::I64)
    }

    pub fn is_unsigned_integer(self) -> bool {
        matches!(self, Self::U8 | Self::U16 | Self::U32 | Self::U64)
    }

    pub fn is_integer(self) -> bool {
        self.is_signed_integer() || self.is_unsigned_integer()
    }

    pub fn is_float(self) -> bool {
        matches!(self, Self::F32 | Self::F64)
    }

    pub fn is_numeric(self) -> bool {
        self.is_integer() || self.is_float()
    }

    pub fn is_orderable(self) -> bool {
        self.is_numeric() || matches!(self, Self::Text | Self::Bool)
    }

    pub fn tag(self) -> u8 {
        match self {
            Self::I64 => 0,
            Self::Bool => 1,
            Self::Text => 2,
            Self::I8 => 3,
            Self::I16 => 4,
            Self::I32 => 5,
            Self::U8 => 6,
            Self::U16 => 7,
            Self::U32 => 8,
            Self::U64 => 9,
            Self::Bytes => 10,
            Self::Uuid => 11,
            Self::F32 => 12,
            Self::F64 => 13,
        }
    }

    pub fn from_tag(tag: u8) -> Option<Self> {
        Some(match tag {
            0 => Self::I64,
            1 => Self::Bool,
            2 => Self::Text,
            3 => Self::I8,
            4 => Self::I16,
            5 => Self::I32,
            6 => Self::U8,
            7 => Self::U16,
            8 => Self::U32,
            9 => Self::U64,
            10 => Self::Bytes,
            11 => Self::Uuid,
            12 => Self::F32,
            13 => Self::F64,
            _ => return None,
        })
    }
}

impl std::fmt::Display for FieldType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::I8 => "i8",
            Self::I16 => "i16",
            Self::I32 => "i32",
            Self::I64 => "i64",
            Self::U8 => "u8",
            Self::U16 => "u16",
            Self::U32 => "u32",
            Self::U64 => "u64",
            Self::F32 => "f32",
            Self::F64 => "f64",
            Self::Bool => "bool",
            Self::Text => "text",
            Self::Bytes => "bytes",
            Self::Uuid => "uuid",
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Column {
    pub name: &'static str,
    pub ty: FieldType,
    pub primary_key: bool,
    pub nullable: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableHandle {
    name: &'static str,
}

impl TableHandle {
    pub const fn new(name: &'static str) -> Self {
        Self { name }
    }

    pub const fn name(self) -> &'static str {
        self.name
    }

    pub fn join_on(self, predicate: crate::query::Predicate) -> crate::query::Join {
        let _ = predicate;
        crate::query::Join::new()
    }
}

pub trait Table: Clone + Send + Sync + 'static {
    type Key: Clone + Send + Sync + 'static;
    type Patch: Clone + std::fmt::Debug + Default + Send + Sync + 'static;

    const NAME: &'static str;
    const COLUMNS: &'static [Column];

    fn key(&self) -> &Self::Key;

    fn to_values(&self) -> Vec<Value>;

    fn from_values(values: Vec<Value>) -> Result<Self>;

    fn key_to_value(key: &Self::Key) -> Value;

    fn key_from_value(value: Value) -> Result<Self::Key>;

    fn apply_patch(&mut self, patch: &Self::Patch);
}

pub trait Projection: Clone + Send + Sync + 'static {
    const FIELDS: &'static [Column];

    fn from_values(values: Vec<Value>) -> Result<Self>;

    fn to_values(&self) -> Vec<Value>;
}

#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Value {
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    F32(f32),
    F64(f64),
    Bool(bool),
    Text(String),
    Bytes(Vec<u8>),
    Uuid([u8; 16]),
    Null(FieldType),
}

impl Value {
    pub fn field_type(&self) -> FieldType {
        match self {
            Self::I8(_) => FieldType::I8,
            Self::I16(_) => FieldType::I16,
            Self::I32(_) => FieldType::I32,
            Self::I64(_) => FieldType::I64,
            Self::U8(_) => FieldType::U8,
            Self::U16(_) => FieldType::U16,
            Self::U32(_) => FieldType::U32,
            Self::U64(_) => FieldType::U64,
            Self::F32(_) => FieldType::F32,
            Self::F64(_) => FieldType::F64,
            Self::Bool(_) => FieldType::Bool,
            Self::Text(_) => FieldType::Text,
            Self::Bytes(_) => FieldType::Bytes,
            Self::Uuid(_) => FieldType::Uuid,
            Self::Null(ty) => *ty,
        }
    }
}

#[doc(hidden)]
pub trait TableOps<Store: ?Sized>: Table {
    fn insert(store: &mut Store, row: Self) -> Result<()>;
    fn insert_many(store: &mut Store, rows: Vec<Self>) -> Result<()>;
    fn upsert(store: &mut Store, row: Self) -> Result<bool>;
    fn upsert_many(store: &mut Store, rows: Vec<Self>) -> Result<u64>;
    fn get(store: &Store, key: Self::Key) -> Result<Option<Self>>;
    fn contains(store: &Store, key: &Self::Key) -> Result<bool>;
    fn update(store: &mut Store, row: Self) -> Result<bool>;
    fn patch(store: &mut Store, key: Self::Key, patch: Self::Patch) -> Result<bool>;
    fn delete(store: &mut Store, key: Self::Key) -> Result<bool>;
    fn count(store: &Store) -> Result<u64>;
    fn scan(store: &Store) -> Result<Vec<Self>>;
}

#[doc(hidden)]
pub trait WireTable: Table {
    fn encode_row(&self) -> Vec<Value>;
    fn decode_row(values: Vec<Value>) -> Result<Self>;
    fn wire_insert<R: crate::wire::Requester>(requester: &mut R, row: Self) -> Result<()>;
    fn wire_insert_many<R: crate::wire::Requester>(
        requester: &mut R,
        rows: Vec<Self>,
    ) -> Result<()>;
    fn wire_upsert<R: crate::wire::Requester>(requester: &mut R, row: Self) -> Result<bool>;
    fn wire_upsert_many<R: crate::wire::Requester>(
        requester: &mut R,
        rows: Vec<Self>,
    ) -> Result<u64>;
    fn wire_get<R: crate::wire::Requester>(
        requester: &mut R,
        key: Self::Key,
    ) -> Result<Option<Self>>;
    fn wire_contains<R: crate::wire::Requester>(requester: &mut R, key: &Self::Key)
        -> Result<bool>;
    fn wire_update<R: crate::wire::Requester>(requester: &mut R, row: Self) -> Result<bool>;
    fn wire_patch<R: crate::wire::Requester>(
        requester: &mut R,
        key: Self::Key,
        patch: Self::Patch,
    ) -> Result<bool>;
    fn wire_delete<R: crate::wire::Requester>(requester: &mut R, key: Self::Key) -> Result<bool>;
    fn wire_count<R: crate::wire::Requester>(requester: &mut R) -> Result<u64>;
    fn wire_scan<R: crate::wire::Requester>(requester: &mut R) -> Result<Vec<Self>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_tags_round_trip() {
        for tag in 0..=13 {
            assert_eq!(FieldType::from_tag(tag).unwrap().tag(), tag);
        }
        assert_eq!(FieldType::from_tag(14), None);
    }

    #[test]
    fn numeric_classification_is_exhaustive() {
        for ty in [
            FieldType::I8,
            FieldType::I16,
            FieldType::I32,
            FieldType::I64,
            FieldType::U8,
            FieldType::U16,
            FieldType::U32,
            FieldType::U64,
        ] {
            assert!(ty.is_integer());
            assert!(ty.is_numeric());
        }
        for ty in [FieldType::F32, FieldType::F64] {
            assert!(ty.is_float());
            assert!(ty.is_numeric());
        }
        for ty in [
            FieldType::Bool,
            FieldType::Text,
            FieldType::Bytes,
            FieldType::Uuid,
        ] {
            assert!(!ty.is_numeric());
        }
    }
}
