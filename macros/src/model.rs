use lightshadedb_codegen::ir::Ty;
use proc_macro2::Span;
use syn::spanned::Spanned;
use syn::{
    Attribute, Expr, Field, Fields, Ident, Item, ItemMod, ItemStruct, ItemTrait, Lit, LitStr, Meta,
    TraitItem, TraitItemFn, Type, Visibility,
};

pub struct Schema {
    pub module: ItemMod,
    pub tables: Vec<Table>,
    pub projections: Vec<Projection>,
    pub operations: Option<OperationTrait>,
}

pub struct Table {
    pub ident: Ident,
    pub db_name: String,
    pub fields: Vec<ModelField>,
    pub primary_key: usize,
    pub rename_from: Option<String>,
    pub ttl_secs: Option<u64>,
}

pub struct Projection {
    pub ident: Ident,
    pub fields: Vec<ModelField>,
}

pub struct ModelField {
    pub ident: Ident,
    pub ty: Type,
    pub ir: Ty,
    pub primary_key: bool,
    pub nullable: bool,
    pub default: Option<Expr>,
    pub rename_from: Option<Ident>,
    pub convert_from: Option<(Type, Ident)>,
    pub span: Span,
}

pub struct OperationTrait {
    pub ident: Ident,
    pub methods: Vec<Operation>,
}

pub struct Operation {
    pub method: TraitItemFn,
    pub kind: OperationKind,
    pub params: Vec<Param>,
}

#[derive(Clone)]
pub struct Param {
    pub ident: Ident,
    pub ty: Type,
    pub ir: Ty,
}

#[derive(Clone)]
pub enum OperationKind {
    Query(QueryArgs),
    Count(QueryArgs),
    Update(UpdateArgs),
    Delete(DeleteArgs),
}

#[derive(Clone, Default)]
pub struct QueryArgs {
    pub from: Option<Type>,
    pub joins: Vec<Expr>,
    pub select: Option<Expr>,
    pub group_by: Vec<Expr>,
    pub filter: Option<Expr>,
    pub order: Vec<Expr>,
    pub limit: Option<Expr>,
    pub offset: Option<Expr>,
}

#[derive(Clone, Default)]
pub struct UpdateArgs {
    pub filter: Option<Expr>,
    pub set: Vec<Expr>,
}

#[derive(Clone, Default)]
pub struct DeleteArgs {
    pub filter: Option<Expr>,
}

pub fn parse_schema(mut module: ItemMod) -> syn::Result<Schema> {
    let module_span = module.span();
    let content = module
        .content
        .as_mut()
        .ok_or_else(|| syn::Error::new(module_span, "schema module must have a body"))?;

    let mut tables = Vec::new();
    let mut projections = Vec::new();
    let mut operations = None;
    let mut errors: Option<syn::Error> = None;

    for item in &content.1 {
        match item {
            Item::Struct(item) => match parse_struct(item) {
                Ok(Some(ParsedStruct::Table(table))) => tables.push(table),
                Ok(Some(ParsedStruct::Projection(projection))) => projections.push(projection),
                Ok(None) => {}
                Err(error) => combine(&mut errors, error),
            },
            Item::Trait(item) if is_operations_trait(item) => {
                if operations.is_some() {
                    combine(
                        &mut errors,
                        syn::Error::new(
                            item.span(),
                            "a schema module may declare only one operations trait",
                        ),
                    );
                    continue;
                }
                match parse_operations(item) {
                    Ok(trait_) => operations = Some(trait_),
                    Err(error) => combine(&mut errors, error),
                }
            }
            _ => {}
        }
    }

    if let Some(error) = errors {
        return Err(error);
    }

    if tables.is_empty() {
        return Err(syn::Error::new(
            module.span(),
            "schema module must contain at least one table struct",
        ));
    }

    let mut seen = std::collections::HashSet::new();
    for table in &tables {
        if !seen.insert(table.db_name.clone()) {
            return Err(syn::Error::new(
                table.ident.span(),
                format!("duplicate table `{}`", table.db_name),
            ));
        }
        validate_reserved_table_name(&table.ident)?;
    }

    Ok(Schema {
        module,
        tables,
        projections,
        operations,
    })
}

enum ParsedStruct {
    Table(Table),
    Projection(Projection),
}

struct StructAttrs {
    skip: bool,
    table: Option<String>,
    rename_from: Option<String>,
    ttl_secs: Option<u64>,
}

fn parse_struct(item: &ItemStruct) -> syn::Result<Option<ParsedStruct>> {
    let attrs = struct_attrs(&item.attrs)?;
    let Fields::Named(named) = &item.fields else {
        return Err(syn::Error::new(
            item.fields.span(),
            "Lightshade DB structs must have named fields",
        ));
    };
    if !matches!(item.vis, Visibility::Public(_)) {
        return Err(syn::Error::new(
            item.vis.span(),
            "Lightshade DB table and projection structs must be `pub`",
        ));
    }
    if !item.generics.params.is_empty() {
        return Err(syn::Error::new(
            item.generics.span(),
            "Lightshade DB structs cannot have generics or lifetimes",
        ));
    }

    let mut fields = Vec::new();
    for field in &named.named {
        if !matches!(field.vis, Visibility::Public(_)) {
            return Err(syn::Error::new(
                field.vis.span(),
                "Lightshade DB fields must be `pub`",
            ));
        }
        let ident = field
            .ident
            .clone()
            .ok_or_else(|| syn::Error::new(field.span(), "named field is missing an identifier"))?;
        let (ir, nullable) = field_type_to_ir(&field.ty).ok_or_else(|| {
            syn::Error::new(
                field.ty.span(),
                format!(
                    "unsupported Lightshade DB field type for `{ident}`; use integers, floats, bool, String, Vec<u8>, [u8; 16], or Option<T> of those",
                ),
            )
        })?;
        let field_attrs = field_attrs(field)?;
        if nullable && field_attrs.primary_key {
            return Err(syn::Error::new(
                field.ty.span(),
                format!(
                    "primary key `{ident}` cannot be nullable; primary keys always hold a value"
                ),
            ));
        }
        fields.push(ModelField {
            ident,
            ty: field.ty.clone(),
            ir,
            primary_key: field_attrs.primary_key,
            nullable,
            default: field_attrs.default,
            rename_from: field_attrs.rename_from,
            convert_from: field_attrs.convert_from,
            span: field.span(),
        });
    }

    if attrs.skip {
        return Ok(Some(ParsedStruct::Projection(Projection {
            ident: item.ident.clone(),
            fields,
        })));
    }

    let primary_keys = fields.iter().filter(|field| field.primary_key).count();
    if primary_keys != 1 {
        return Err(syn::Error::new(
            item.ident.span(),
            format!(
                "table `{}` must have exactly one `#[lightshadedb(primary_key)]` field",
                item.ident
            ),
        ));
    }
    let primary_key = fields
        .iter()
        .position(|field| field.primary_key)
        .expect("checked");
    if fields[primary_key].default.is_some() {
        return Err(syn::Error::new(
            fields[primary_key].span,
            "a primary key cannot have a default",
        ));
    }

    let db_name = attrs
        .table
        .unwrap_or_else(|| snake_case(&item.ident.to_string()));
    if let Some(ttl_secs) = attrs.ttl_secs {
        if attrs.skip {
            return Err(syn::Error::new(
                item.ident.span(),
                "ttl_secs cannot be used on a skipped projection struct",
            ));
        }
        let expires = fields.iter().find(|field| field.ident == "expires_at");
        match expires {
            Some(field) => {
                if field.ir != Ty::I64 || field.nullable {
                    return Err(syn::Error::new(
                        field.span,
                        "ttl_secs tables need a non-nullable expires_at i64 field holding absolute milliseconds",
                    ));
                }
                if field.primary_key {
                    return Err(syn::Error::new(
                        field.span,
                        "ttl_secs expiry field expires_at cannot be the primary key",
                    ));
                }
            }
            None => {
                return Err(syn::Error::new(
                    item.ident.span(),
                    "ttl_secs tables need a non-nullable expires_at i64 field holding absolute milliseconds",
                ));
            }
        }
        let _ = ttl_secs;
    }
    Ok(Some(ParsedStruct::Table(Table {
        ident: item.ident.clone(),
        db_name,
        fields,
        primary_key,
        rename_from: attrs.rename_from,
        ttl_secs: attrs.ttl_secs,
    })))
}

struct FieldAttrs {
    primary_key: bool,
    default: Option<Expr>,
    rename_from: Option<Ident>,
    convert_from: Option<(Type, Ident)>,
}

fn field_attrs(field: &Field) -> syn::Result<FieldAttrs> {
    let mut attrs = FieldAttrs {
        primary_key: false,
        default: None,
        rename_from: None,
        convert_from: None,
    };
    let mut convert_from: Option<Type> = None;
    let mut with: Option<Ident> = None;
    if let Some(attr) = field
        .attrs
        .iter()
        .find(|attr| attr.path().is_ident("lightshadedb"))
    {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("primary_key") {
                attrs.primary_key = true;
                return Ok(());
            }
            if meta.path.is_ident("default") {
                let value = meta.value()?;
                attrs.default = Some(value.parse()?);
                return Ok(());
            }
            if meta.path.is_ident("rename_from") {
                attrs.rename_from = Some(meta.value()?.parse()?);
                return Ok(());
            }
            if meta.path.is_ident("convert_from") {
                convert_from = Some(meta.value()?.parse()?);
                return Ok(());
            }
            if meta.path.is_ident("with") {
                with = Some(meta.value()?.parse()?);
                return Ok(());
            }
            Err(meta.error("unknown field attribute"))
        })?;
    }
    match (convert_from, with) {
        (Some(old), Some(function)) => attrs.convert_from = Some((old, function)),
        (Some(_), None) => {
            return Err(syn::Error::new(
                field.span(),
                "`convert_from` requires `with = function`",
            ))
        }
        (None, Some(_)) => {
            return Err(syn::Error::new(
                field.span(),
                "`with` is only valid with `convert_from`",
            ))
        }
        (None, None) => {}
    }
    Ok(attrs)
}

fn struct_attrs(attrs: &[Attribute]) -> syn::Result<StructAttrs> {
    let mut parsed = StructAttrs {
        skip: false,
        table: None,
        rename_from: None,
        ttl_secs: None,
    };
    for attr in attrs {
        if !attr.path().is_ident("lightshadedb") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("skip") {
                parsed.skip = true;
                return Ok(());
            }
            if meta.path.is_ident("table") {
                parsed.table = Some(meta.value()?.parse::<LitStr>()?.value());
                return Ok(());
            }
            if meta.path.is_ident("rename_from") {
                parsed.rename_from = Some(meta.value()?.parse::<LitStr>()?.value());
                return Ok(());
            }
            if meta.path.is_ident("ttl_secs") {
                let lit: syn::LitInt = meta.value()?.parse()?;
                let value = lit.base10_parse::<u64>().map_err(|_| {
                    meta.error("ttl_secs must be a positive integer number of seconds")
                })?;
                if value == 0 {
                    return Err(meta.error("ttl_secs must be greater than zero"));
                }
                parsed.ttl_secs = Some(value);
                return Ok(());
            }
            Err(meta.error("unknown struct attribute"))
        })?;
    }
    Ok(parsed)
}

fn field_type_to_ir(ty: &Type) -> Option<(Ty, bool)> {
    if let Type::Path(path) = ty {
        if let Some(segment) = path.path.segments.last() {
            if segment.ident == "Option" {
                let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
                    return None;
                };
                if arguments.args.len() != 1 {
                    return None;
                }
                let syn::GenericArgument::Type(inner) = &arguments.args[0] else {
                    return None;
                };
                if type_is(inner, "Option") {
                    return None;
                }
                return Some((type_to_ir(inner)?, true));
            }
        }
    }
    Some((type_to_ir(ty)?, false))
}

pub fn type_to_ir(ty: &Type) -> Option<Ty> {
    match ty {
        Type::Path(path) => {
            let segment = path.path.segments.last()?;
            let ident = segment.ident.to_string();
            Some(match ident.as_str() {
                "i8" => Ty::I8,
                "i16" => Ty::I16,
                "i32" => Ty::I32,
                "i64" => Ty::I64,
                "u8" => Ty::U8,
                "u16" => Ty::U16,
                "u32" => Ty::U32,
                "u64" => Ty::U64,
                "f32" => Ty::F32,
                "f64" => Ty::F64,
                "bool" => Ty::Bool,
                "String" => Ty::Text,
                "Vec" => {
                    let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
                        return None;
                    };
                    let inner = arguments.args.first()?;
                    match inner {
                        syn::GenericArgument::Type(Type::Path(inner))
                            if inner.path.is_ident("u8") =>
                        {
                            Ty::Bytes
                        }
                        _ => return None,
                    }
                }
                _ => return None,
            })
        }
        Type::Array(array) => {
            let syn::Type::Path(element) = &*array.elem else {
                return None;
            };
            if !element.path.is_ident("u8") {
                return None;
            }
            let syn::Expr::Lit(syn::ExprLit {
                lit: Lit::Int(length),
                ..
            }) = &array.len
            else {
                return None;
            };
            (length.base10_parse::<usize>().ok()? == 16).then_some(Ty::Uuid)
        }
        _ => None,
    }
}

fn is_operations_trait(item: &ItemTrait) -> bool {
    item.items.iter().any(|member| {
        let TraitItem::Fn(method) = member else {
            return false;
        };
        method.attrs.iter().any(is_operation_attr)
    })
}

fn is_operation_attr(attr: &Attribute) -> bool {
    let Some(segment) = attr.path().segments.last() else {
        return false;
    };
    matches!(
        segment.ident.to_string().as_str(),
        "query" | "update" | "delete"
    )
}

fn parse_operations(item: &ItemTrait) -> syn::Result<OperationTrait> {
    let mut methods = Vec::new();
    for member in &item.items {
        let TraitItem::Fn(method) = member else {
            continue;
        };
        let Some(attr) = method.attrs.iter().find(|attr| is_operation_attr(attr)) else {
            return Err(syn::Error::new(
                method.span(),
                "every method in an operations trait needs a `#[lightshadedb::query]`, `#[lightshadedb::update]`, or `#[lightshadedb::delete]` attribute",
            ));
        };
        methods.push(parse_operation(method, attr)?);
    }
    if methods.is_empty() {
        return Err(syn::Error::new(
            item.span(),
            "operations trait must declare at least one method",
        ));
    }
    Ok(OperationTrait {
        ident: item.ident.clone(),
        methods,
    })
}

fn parse_operation(method: &TraitItemFn, attr: &Attribute) -> syn::Result<Operation> {
    if method.sig.asyncness.is_some() {
        return Err(syn::Error::new(
            method.sig.span(),
            "operations cannot be async",
        ));
    }
    if method.sig.generics.params.iter().next().is_some() {
        return Err(syn::Error::new(
            method.sig.generics.span(),
            "operations cannot be generic",
        ));
    }
    if method.default.is_some() {
        return Err(syn::Error::new(
            method.span(),
            "operations cannot have a body",
        ));
    }
    let receiver = method.sig.inputs.iter().find_map(|input| match input {
        syn::FnArg::Receiver(receiver) => Some(receiver),
        syn::FnArg::Typed(_) => None,
    });
    let receiver = receiver.ok_or_else(|| {
        syn::Error::new(
            method.sig.span(),
            "operations need a `&self` or `&mut self` receiver",
        )
    })?;
    if receiver.reference.is_none() {
        return Err(syn::Error::new(
            receiver.span(),
            "operations take `&self` or `&mut self`",
        ));
    }
    let mutable_receiver = receiver.mutability.is_some();

    let mut params = Vec::new();
    for input in &method.sig.inputs {
        let syn::FnArg::Typed(typed) = input else {
            continue;
        };
        let syn::Pat::Ident(pattern) = typed.pat.as_ref() else {
            return Err(syn::Error::new(
                typed.pat.span(),
                "operation parameters must be simple identifiers",
            ));
        };
        let ir = parameter_to_ir(&typed.ty).ok_or_else(|| {
            if type_is(&typed.ty, "Option") {
                syn::Error::new(
                    typed.ty.span(),
                    "operation parameters cannot be Option<...>; bind a concrete value, or declare a separate is_null() query",
                )
            } else {
                syn::Error::new(
                    typed.ty.span(),
                    "unsupported operation parameter type; use a scalar, &str/String, &[u8]/Vec<u8>, or &[u8; 16]/[u8; 16]",
                )
            }
        })?;
        params.push(Param {
            ident: pattern.ident.clone(),
            ty: (*typed.ty).clone(),
            ir,
        });
    }

    let (kind, kind_mutable) = parse_kind(attr)?;
    let wants_mut = matches!(kind, OperationKind::Update(_) | OperationKind::Delete(_));
    if kind_mutable != wants_mut {
        return Err(syn::Error::new(
            receiver.span(),
            if wants_mut {
                "update and delete operations take `&mut self`"
            } else {
                "query operations take `&self`"
            },
        ));
    }
    if mutable_receiver != wants_mut {
        return Err(syn::Error::new(
            receiver.span(),
            if wants_mut {
                "update and delete operations take `&mut self`"
            } else {
                "query operations take `&self`"
            },
        ));
    }

    validate_return_type(&method.sig.output, &kind)?;
    Ok(Operation {
        params,
        method: method.clone(),
        kind,
    })
}

fn parse_kind(attr: &Attribute) -> syn::Result<(OperationKind, bool)> {
    let name = attr
        .path()
        .segments
        .last()
        .expect("operation attr")
        .ident
        .to_string();
    let mut query = QueryArgs::default();
    let mut update = UpdateArgs::default();
    let mut delete = DeleteArgs::default();
    let mut count = false;
    let mut has_filter = false;
    let mut has_set = false;
    let mut seen = std::collections::HashSet::new();

    attr.parse_nested_meta(|meta| {
        if meta.path.is_ident("count") {
            count = true;
            return Ok(());
        }
        let key = meta
            .path
            .get_ident()
            .map(ToString::to_string)
            .ok_or_else(|| meta.error("expected a named argument"))?;
        if !seen.insert(key.clone()) {
            return Err(meta.error(format!("duplicate argument `{key}`")));
        }
        match key.as_str() {
            "from" => query.from = Some(meta.value()?.parse()?),
            "filter" => {
                has_filter = true;
                query.filter = Some(meta.value()?.parse()?);
                update.filter = query.filter.clone();
                delete.filter = query.filter.clone();
            }
            "joins" => query.joins = parse_expr_list(meta.value()?.parse()?)?,
            "select" => query.select = Some(meta.value()?.parse()?),
            "group_by" => query.group_by = parse_expr_list(meta.value()?.parse()?)?,
            "order" => query.order = parse_expr_list(meta.value()?.parse()?)?,
            "limit" => query.limit = Some(meta.value()?.parse()?),
            "offset" => query.offset = Some(meta.value()?.parse()?),
            "set" => {
                has_set = true;
                update.set = parse_expr_list(meta.value()?.parse()?)?;
            }
            other => return Err(meta.error(format!("unknown argument `{other}`"))),
        }
        Ok(())
    })?;

    match name.as_str() {
        "query" => {
            if has_set {
                return Err(syn::Error::new(
                    attr.span(),
                    "`set` is only valid on update",
                ));
            }
            if count && (!query.order.is_empty() || query.limit.is_some() || query.offset.is_some())
            {
                return Err(syn::Error::new(
                    attr.span(),
                    "`count` operations cannot have `order by`, `limit`, or `offset`",
                ));
            }
            if count {
                Ok((OperationKind::Count(query), false))
            } else {
                Ok((OperationKind::Query(query), false))
            }
        }
        "update" => {
            if count
                || query.from.is_some()
                || !query.order.is_empty()
                || query.limit.is_some()
                || query.offset.is_some()
            {
                return Err(syn::Error::new(
                    attr.span(),
                    "update accepts only `filter` and `set`",
                ));
            }
            if !has_set {
                return Err(syn::Error::new(attr.span(), "update requires `set`"));
            }
            Ok((OperationKind::Update(update), true))
        }
        "delete" => {
            if count
                || has_set
                || query.from.is_some()
                || !query.order.is_empty()
                || query.limit.is_some()
                || query.offset.is_some()
            {
                return Err(syn::Error::new(attr.span(), "delete accepts only `filter`"));
            }
            let _ = has_filter;
            Ok((OperationKind::Delete(delete), true))
        }
        _ => unreachable!(),
    }
}

fn parse_expr_list(expr: Expr) -> syn::Result<Vec<Expr>> {
    match expr {
        Expr::Tuple(tuple) => Ok(tuple.elems.into_iter().collect()),
        Expr::Paren(paren) => parse_expr_list(*paren.expr),
        other => Ok(vec![other]),
    }
}

fn validate_return_type(output: &syn::ReturnType, kind: &OperationKind) -> syn::Result<()> {
    let syn::ReturnType::Type(_, ty) = output else {
        return Err(syn::Error::new(
            output.span(),
            "operations must return `lightshadedb::Result<...>`",
        ));
    };
    let Type::Path(path) = ty.as_ref() else {
        return Err(syn::Error::new(
            ty.span(),
            "expected `lightshadedb::Result<T>`",
        ));
    };
    let segment = path
        .path
        .segments
        .last()
        .ok_or_else(|| syn::Error::new(ty.span(), "expected `Result<T>`"))?;
    if segment.ident != "Result" {
        return Err(syn::Error::new(
            ty.span(),
            "expected `lightshadedb::Result<T>`",
        ));
    }
    let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return Err(syn::Error::new(ty.span(), "expected `Result<T>`"));
    };
    let Some(syn::GenericArgument::Type(inner)) = arguments.args.first() else {
        return Err(syn::Error::new(ty.span(), "expected `Result<T>`"));
    };
    match kind {
        OperationKind::Query(_) => {
            if !matches!(inner, Type::Path(path) if path.path.segments.last().is_some_and(|s| s.ident == "Vec"))
            {
                return Err(syn::Error::new(
                    inner.span(),
                    "query operations return `Result<Vec<Row>>`",
                ));
            }
        }
        OperationKind::Count(_) => {
            if !type_is(inner, "i64") {
                return Err(syn::Error::new(
                    inner.span(),
                    "count operations return `Result<i64>`",
                ));
            }
        }
        OperationKind::Update(_) | OperationKind::Delete(_) => {
            if !type_is(inner, "u64") {
                return Err(syn::Error::new(
                    inner.span(),
                    "update and delete operations return `Result<u64>`",
                ));
            }
        }
    }
    Ok(())
}

pub(crate) fn type_is(ty: &Type, name: &str) -> bool {
    matches!(ty, Type::Path(path) if path.path.segments.last().is_some_and(|s| s.ident == name))
}

fn parameter_to_ir(ty: &Type) -> Option<Ty> {
    if let Type::Reference(reference) = ty {
        let inner = reference.elem.as_ref();
        if let Type::Slice(slice) = inner {
            if let Type::Path(element) = slice.elem.as_ref() {
                if element.path.is_ident("u8") {
                    return Some(Ty::Bytes);
                }
            }
            return None;
        }
        if let Type::Path(path) = inner {
            let segment = path.path.segments.last()?;
            return match segment.ident.to_string().as_str() {
                "str" => Some(Ty::Text),
                "u8" => Some(Ty::Bytes),
                _ => None,
            };
        }
        if let Type::Array(array) = inner {
            if let Type::Path(element) = &*array.elem {
                if element.path.is_ident("u8") {
                    if let syn::Expr::Lit(syn::ExprLit {
                        lit: Lit::Int(length),
                        ..
                    }) = &array.len
                    {
                        if length.base10_parse::<usize>().ok()? == 16 {
                            return Some(Ty::Uuid);
                        }
                    }
                }
            }
        }
        return None;
    }
    type_to_ir(ty).or_else(|| {
        if let Type::Array(array) = ty {
            if let Type::Path(element) = &*array.elem {
                if element.path.is_ident("u8") {
                    if let syn::Expr::Lit(syn::ExprLit {
                        lit: Lit::Int(length),
                        ..
                    }) = &array.len
                    {
                        if length.base10_parse::<usize>().ok()? == 16 {
                            return Some(Ty::Uuid);
                        }
                    }
                }
            }
        }
        None
    })
}

fn validate_reserved_table_name(ident: &Ident) -> syn::Result<()> {
    const RESERVED: &[&str] = &[
        "Db",
        "Client",
        "Server",
        "App",
        "AppConfig",
        "Backend",
        "admin",
        "migration_plan",
        "migrate",
        "OpenOptions",
    ];
    if RESERVED.contains(&ident.to_string().as_str()) {
        return Err(syn::Error::new(
            ident.span(),
            format!("`{ident}` is reserved by Lightshade DB"),
        ));
    }
    Ok(())
}

fn snake_case(value: &str) -> String {
    let mut out = String::new();
    for (index, character) in value.chars().enumerate() {
        if character.is_uppercase() {
            if index != 0 {
                out.push('_');
            }
            for lower in character.to_lowercase() {
                out.push(lower);
            }
        } else {
            out.push(character);
        }
    }
    out
}

fn combine(errors: &mut Option<syn::Error>, error: syn::Error) {
    match errors {
        Some(existing) => existing.combine(error),
        None => *errors = Some(error),
    }
}

pub fn strip_lightshadedb_attrs(attrs: &mut Vec<Attribute>) {
    attrs.retain(|attr| {
        !attr.path().is_ident("lightshadedb")
            && !attr.path().segments.last().is_some_and(|segment| {
                matches!(
                    segment.ident.to_string().as_str(),
                    "query" | "update" | "delete"
                )
            })
    });
}

pub fn ensure_derives(item: &mut ItemStruct) {
    let mut has_clone = false;
    let mut has_debug = false;
    for attr in &item.attrs {
        if attr.path().is_ident("derive") {
            if let Meta::List(list) = &attr.meta {
                let text = list.tokens.to_string();
                has_clone |= text.split(',').any(|part| part.trim() == "Clone");
                has_debug |= text.split(',').any(|part| part.trim() == "Debug");
            }
        }
    }
    if has_clone && has_debug {
        return;
    }
    let additions = match (has_clone, has_debug) {
        (true, false) => quote::quote!(Debug),
        (false, true) => quote::quote!(Clone),
        _ => quote::quote!(Clone, Debug),
    };
    if let Some(attr) = item
        .attrs
        .iter_mut()
        .find(|attr| attr.path().is_ident("derive"))
    {
        if let Meta::List(list) = &mut attr.meta {
            let tokens = &list.tokens;
            list.tokens = quote::quote!(#tokens, #additions);
        }
    } else {
        item.attrs.push(syn::parse_quote!(#[derive(#additions)]));
    }
}

pub fn field_path(expr: &Expr) -> Option<(Ident, Ident)> {
    let Expr::Path(path) = expr else {
        return None;
    };
    if path.qself.is_some() {
        return None;
    }
    let mut segments = path.path.segments.iter();
    let first = segments.next()?;
    let second = segments.next()?;
    if segments.next().is_some() {
        return None;
    }
    Some((first.ident.clone(), second.ident.clone()))
}

pub fn literal_for_ty(lit: &Lit, ty: Ty) -> Option<lightshadedb_codegen::ir::Value> {
    use lightshadedb_codegen::ir::Value;
    Some(match (ty, lit) {
        (Ty::I8, Lit::Int(value)) => Value::I8(value.base10_parse().ok()?),
        (Ty::I16, Lit::Int(value)) => Value::I16(value.base10_parse().ok()?),
        (Ty::I32, Lit::Int(value)) => Value::I32(value.base10_parse().ok()?),
        (Ty::I64, Lit::Int(value)) => Value::I64(value.base10_parse().ok()?),
        (Ty::U8, Lit::Int(value)) => Value::U8(value.base10_parse().ok()?),
        (Ty::U16, Lit::Int(value)) => Value::U16(value.base10_parse().ok()?),
        (Ty::U32, Lit::Int(value)) => Value::U32(value.base10_parse().ok()?),
        (Ty::U64, Lit::Int(value)) => Value::U64(value.base10_parse().ok()?),
        (Ty::F32, Lit::Float(value)) => Value::F32(value.base10_parse().ok()?),
        (Ty::F64, Lit::Float(value)) => Value::F64(value.base10_parse().ok()?),
        (Ty::Bool, Lit::Bool(value)) => Value::Bool(value.value),
        (Ty::Text, Lit::Str(value)) => Value::Text(value.value()),
        (Ty::Bytes, Lit::ByteStr(value)) => Value::Bytes(value.value()),
        _ => return None,
    })
}
