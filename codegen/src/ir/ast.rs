use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Ty {
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

impl Ty {
    pub fn parse(text: &str) -> Option<Ty> {
        match text {
            "i8" => Some(Ty::I8),
            "i16" => Some(Ty::I16),
            "i32" => Some(Ty::I32),
            "i64" => Some(Ty::I64),
            "u8" => Some(Ty::U8),
            "u16" => Some(Ty::U16),
            "u32" => Some(Ty::U32),
            "u64" => Some(Ty::U64),
            "f32" => Some(Ty::F32),
            "f64" => Some(Ty::F64),
            "bool" => Some(Ty::Bool),
            "text" => Some(Ty::Text),
            "bytes" => Some(Ty::Bytes),
            "uuid" => Some(Ty::Uuid),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Ty::I8 => "i8",
            Ty::I16 => "i16",
            Ty::I32 => "i32",
            Ty::I64 => "i64",
            Ty::U8 => "u8",
            Ty::U16 => "u16",
            Ty::U32 => "u32",
            Ty::U64 => "u64",
            Ty::F32 => "f32",
            Ty::F64 => "f64",
            Ty::Bool => "bool",
            Ty::Text => "text",
            Ty::Bytes => "bytes",
            Ty::Uuid => "uuid",
        }
    }

    pub fn rust(self) -> &'static str {
        match self {
            Ty::I8 => "i8",
            Ty::I16 => "i16",
            Ty::I32 => "i32",
            Ty::I64 => "i64",
            Ty::U8 => "u8",
            Ty::U16 => "u16",
            Ty::U32 => "u32",
            Ty::U64 => "u64",
            Ty::F32 => "f32",
            Ty::F64 => "f64",
            Ty::Bool => "bool",
            Ty::Text => "String",
            Ty::Bytes => "Vec<u8>",
            Ty::Uuid => "[u8; 16]",
        }
    }

    pub fn is_integer(self) -> bool {
        matches!(
            self,
            Ty::I8 | Ty::I16 | Ty::I32 | Ty::I64 | Ty::U8 | Ty::U16 | Ty::U32 | Ty::U64
        )
    }

    pub fn is_signed_integer(self) -> bool {
        matches!(self, Ty::I8 | Ty::I16 | Ty::I32 | Ty::I64)
    }

    pub fn is_unsigned_integer(self) -> bool {
        matches!(self, Ty::U8 | Ty::U16 | Ty::U32 | Ty::U64)
    }

    pub fn is_float(self) -> bool {
        matches!(self, Ty::F32 | Ty::F64)
    }

    pub fn is_numeric(self) -> bool {
        self.is_integer() || self.is_float()
    }

    pub fn integer_zero(self) -> Option<i64> {
        self.is_integer().then_some(0)
    }

    pub fn integer_bounds(self) -> Option<(i64, i64)> {
        Some(match self {
            Ty::I8 => (i8::MIN as i64, i8::MAX as i64),
            Ty::I16 => (i16::MIN as i64, i16::MAX as i64),
            Ty::I32 => (i32::MIN as i64, i32::MAX as i64),
            Ty::I64 => (i64::MIN, i64::MAX),
            Ty::U8 => (0, u8::MAX as i64),
            Ty::U16 => (0, u16::MAX as i64),
            Ty::U32 => (0, u32::MAX as i64),
            _ => return None,
        })
    }

    pub fn integer_fits(self, value: i64) -> bool {
        match self {
            Ty::I64 => true,
            Ty::U64 => value >= 0,
            _ => self
                .integer_bounds()
                .map(|(minimum, maximum)| (minimum..=maximum).contains(&value))
                .unwrap_or(false),
        }
    }
}

impl fmt::Display for Ty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Clone, Debug)]
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
    Null(Ty),
}

fn value_rank(value: &Value) -> u8 {
    match value {
        Value::I8(_) => 0,
        Value::I16(_) => 1,
        Value::I32(_) => 2,
        Value::I64(_) => 3,
        Value::U8(_) => 4,
        Value::U16(_) => 5,
        Value::U32(_) => 6,
        Value::U64(_) => 7,
        Value::F32(_) => 8,
        Value::F64(_) => 9,
        Value::Bool(_) => 10,
        Value::Text(_) => 11,
        Value::Bytes(_) => 12,
        Value::Uuid(_) => 13,
        Value::Null(_) => 14,
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Value {}

impl PartialOrd for Value {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Value {
    fn cmp(&self, other: &Self) -> Ordering {
        let rank = value_rank(self).cmp(&value_rank(other));
        if rank != Ordering::Equal {
            return rank;
        }
        match (self, other) {
            (Value::I8(left), Value::I8(right)) => left.cmp(right),
            (Value::I16(left), Value::I16(right)) => left.cmp(right),
            (Value::I32(left), Value::I32(right)) => left.cmp(right),
            (Value::I64(left), Value::I64(right)) => left.cmp(right),
            (Value::U8(left), Value::U8(right)) => left.cmp(right),
            (Value::U16(left), Value::U16(right)) => left.cmp(right),
            (Value::U32(left), Value::U32(right)) => left.cmp(right),
            (Value::U64(left), Value::U64(right)) => left.cmp(right),
            (Value::F32(left), Value::F32(right)) => left.total_cmp(right),
            (Value::F64(left), Value::F64(right)) => left.total_cmp(right),
            (Value::Bool(left), Value::Bool(right)) => left.cmp(right),
            (Value::Text(left), Value::Text(right)) => left.cmp(right),
            (Value::Bytes(left), Value::Bytes(right)) => left.cmp(right),
            (Value::Uuid(left), Value::Uuid(right)) => left.cmp(right),
            (Value::Null(_), Value::Null(_)) => Ordering::Equal,
            _ => Ordering::Equal,
        }
    }
}

impl Hash for Value {
    fn hash<H: Hasher>(&self, state: &mut H) {
        value_rank(self).hash(state);
        match self {
            Value::I8(value) => value.hash(state),
            Value::I16(value) => value.hash(state),
            Value::I32(value) => value.hash(state),
            Value::I64(value) => value.hash(state),
            Value::U8(value) => value.hash(state),
            Value::U16(value) => value.hash(state),
            Value::U32(value) => value.hash(state),
            Value::U64(value) => value.hash(state),
            Value::F32(value) => value.to_bits().hash(state),
            Value::F64(value) => value.to_bits().hash(state),
            Value::Bool(value) => value.hash(state),
            Value::Text(value) => value.hash(state),
            Value::Bytes(value) => value.hash(state),
            Value::Uuid(value) => value.hash(state),
            Value::Null(ty) => (*ty as u8).hash(state),
        }
    }
}

impl Value {
    pub fn ty(&self) -> Ty {
        match self {
            Value::I8(_) => Ty::I8,
            Value::I16(_) => Ty::I16,
            Value::I32(_) => Ty::I32,
            Value::I64(_) => Ty::I64,
            Value::U8(_) => Ty::U8,
            Value::U16(_) => Ty::U16,
            Value::U32(_) => Ty::U32,
            Value::U64(_) => Ty::U64,
            Value::F32(_) => Ty::F32,
            Value::F64(_) => Ty::F64,
            Value::Bool(_) => Ty::Bool,
            Value::Text(_) => Ty::Text,
            Value::Bytes(_) => Ty::Bytes,
            Value::Uuid(_) => Ty::Uuid,
            Value::Null(ty) => *ty,
        }
    }

    pub fn coerce_to(self, ty: Ty) -> Option<Value> {
        if self.ty() == ty {
            return Some(self);
        }
        match self {
            Value::I64(value) => Value::cast_integer(value, ty).or(match ty {
                Ty::F32 => Some(Value::F32(value as f32)),
                Ty::F64 => Some(Value::F64(value as f64)),
                _ => None,
            }),
            Value::U64(value) => Value::cast_unsigned(value, ty).or(match ty {
                Ty::F32 => Some(Value::F32(value as f32)),
                Ty::F64 => Some(Value::F64(value as f64)),
                _ => None,
            }),
            Value::F64(value) if ty == Ty::F32 => {
                let narrowed = value as f32;
                if value.is_finite() && !narrowed.is_finite() {
                    None
                } else {
                    Some(Value::F32(narrowed))
                }
            }
            Value::F32(value) if ty == Ty::F64 => Some(Value::F64(value as f64)),
            _ => None,
        }
    }

    pub fn cast_integer(value: i64, ty: Ty) -> Option<Value> {
        if !ty.integer_fits(value) {
            return None;
        }
        Some(match ty {
            Ty::I8 => Value::I8(value as i8),
            Ty::I16 => Value::I16(value as i16),
            Ty::I32 => Value::I32(value as i32),
            Ty::I64 => Value::I64(value),
            Ty::U8 => Value::U8(value as u8),
            Ty::U16 => Value::U16(value as u16),
            Ty::U32 => Value::U32(value as u32),
            Ty::U64 => Value::U64(value as u64),
            _ => return None,
        })
    }

    pub fn cast_unsigned(value: u64, ty: Ty) -> Option<Value> {
        Some(match ty {
            Ty::I8 => Value::I8(i8::try_from(value).ok()?),
            Ty::I16 => Value::I16(i16::try_from(value).ok()?),
            Ty::I32 => Value::I32(i32::try_from(value).ok()?),
            Ty::I64 => Value::I64(i64::try_from(value).ok()?),
            Ty::U8 => Value::U8(u8::try_from(value).ok()?),
            Ty::U16 => Value::U16(u16::try_from(value).ok()?),
            Ty::U32 => Value::U32(u32::try_from(value).ok()?),
            Ty::U64 => Value::U64(value),
            _ => return None,
        })
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::I8(v) => write!(f, "{v}"),
            Value::I16(v) => write!(f, "{v}"),
            Value::I32(v) => write!(f, "{v}"),
            Value::I64(v) => write!(f, "{v}"),
            Value::U8(v) => write!(f, "{v}"),
            Value::U16(v) => write!(f, "{v}"),
            Value::U32(v) => write!(f, "{v}"),
            Value::U64(v) => write!(f, "{v}"),
            Value::F32(v) => write!(f, "{v}"),
            Value::F64(v) => write!(f, "{v}"),
            Value::Bool(v) => write!(f, "{v}"),
            Value::Text(v) => write!(f, "{v}"),
            Value::Bytes(v) => {
                for byte in v {
                    write!(f, "{byte:02x}")?;
                }
                Ok(())
            }
            Value::Uuid(v) => {
                for (index, byte) in v.iter().enumerate() {
                    if matches!(index, 4 | 6 | 8 | 10) {
                        write!(f, "-")?;
                    }
                    write!(f, "{byte:02x}")?;
                }
                Ok(())
            }
            Value::Null(_) => write!(f, "NULL"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Field {
    pub name: String,
    pub ty: Ty,
    pub primary_key: bool,
    pub default: Option<Value>,
    pub nullable: bool,
}

#[derive(Clone, Debug)]
pub struct Table {
    pub name: String,
    pub fields: Vec<Field>,
}

impl Table {
    pub fn field(&self, name: &str) -> Option<(usize, &Field)> {
        self.fields
            .iter()
            .enumerate()
            .find(|(_, field)| field.name == name)
    }

    pub fn primary_key_index(&self) -> Option<usize> {
        self.fields.iter().position(|field| field.primary_key)
    }

    pub fn primary_key(&self) -> Option<&Field> {
        self.primary_key_index().map(|index| &self.fields[index])
    }
}

#[derive(Clone, Debug)]
pub struct Param {
    pub name: String,
    pub ty: Ty,
    pub nullable: bool,
}

#[derive(Clone, Debug)]
pub enum Operand {
    Field(String),
    Param(String),
    Literal(Value),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cmp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Clone, Debug)]
pub enum Expr {
    Cmp {
        left: Operand,
        op: Cmp,
        right: Operand,
    },
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    IsNull {
        field: FieldRef,
    },
    IsNotNull {
        field: FieldRef,
    },
}

#[derive(Clone, Debug)]
pub enum Limit {
    Literal(u64),
    Param(String),
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FieldRef {
    pub qualifier: Option<String>,
    pub field: String,
}

impl FieldRef {
    pub fn bare(field: impl Into<String>) -> FieldRef {
        FieldRef {
            qualifier: None,
            field: field.into(),
        }
    }

    pub fn parse(text: &str) -> Option<FieldRef> {
        if let Some((qualifier, field)) = text.split_once('.') {
            if qualifier.is_empty() || field.is_empty() || field.contains('.') {
                return None;
            }
            Some(FieldRef {
                qualifier: Some(qualifier.to_string()),
                field: field.to_string(),
            })
        } else if text.is_empty() {
            None
        } else {
            Some(FieldRef::bare(text))
        }
    }
}

impl fmt::Display for FieldRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.qualifier {
            Some(qualifier) => write!(f, "{qualifier}.{}", self.field),
            None => f.write_str(&self.field),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Join {
    pub table: String,
    pub alias: Option<String>,
    pub left: FieldRef,
    pub right: FieldRef,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Aggregate {
    Count,
    Sum,
    Min,
    Max,
    Avg,
}

impl Aggregate {
    pub fn name(self) -> &'static str {
        match self {
            Aggregate::Count => "count",
            Aggregate::Sum => "sum",
            Aggregate::Min => "min",
            Aggregate::Max => "max",
            Aggregate::Avg => "avg",
        }
    }

    pub fn requires_field(self) -> bool {
        !matches!(self, Aggregate::Count)
    }
}

#[derive(Clone, Debug)]
pub enum SelectExpr {
    Field(FieldRef),
    Aggregate {
        function: Aggregate,
        field: Option<FieldRef>,
    },
}

#[derive(Clone, Debug)]
pub struct SelectItem {
    pub expr: SelectExpr,
    pub alias: String,
}

#[derive(Clone, Debug)]
pub enum Returns {
    None,
    One(Ty),
    Many(String),
}

#[derive(Clone, Debug)]
pub enum Body {
    Query {
        table: String,
        alias: Option<String>,
        joins: Vec<Join>,
        select: Vec<SelectItem>,
        group_by: Vec<FieldRef>,
        filter: Option<Expr>,
        order: Vec<(String, bool)>,
        limit: Option<Limit>,
        offset: Option<Limit>,
        count: bool,
    },
    Update {
        table: String,
        sets: Vec<(String, Operand)>,
        filter: Option<Expr>,
    },
    Delete {
        table: String,
        filter: Option<Expr>,
    },
    Insert {
        table: String,
    },
}

#[derive(Clone, Debug)]
pub struct Operation {
    pub name: String,
    pub params: Vec<Param>,
    pub returns: Returns,
    pub body: Body,
}

#[derive(Clone, Debug, Default)]
pub struct Workload {
    pub tables: Vec<Table>,
    pub projections: Vec<String>,
    pub operations: Vec<Operation>,
}

impl Workload {
    pub fn table(&self, name: &str) -> Option<&Table> {
        self.tables.iter().find(|table| table.name == name)
    }

    pub fn operation(&self, name: &str) -> Option<&Operation> {
        self.operations
            .iter()
            .find(|operation| operation.name == name)
    }

    pub fn is_projection(&self, name: &str) -> bool {
        self.projections.iter().any(|projection| projection == name)
    }
}

pub type Row = Vec<Value>;
