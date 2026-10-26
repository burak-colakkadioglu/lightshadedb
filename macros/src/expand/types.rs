use lightshadedb_codegen::ir::Ty;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::Ident;

pub(crate) fn field_type_tokens(ty: Ty) -> TokenStream {
    match ty {
        Ty::I8 => quote!(::lightshadedb::FieldType::I8),
        Ty::I16 => quote!(::lightshadedb::FieldType::I16),
        Ty::I32 => quote!(::lightshadedb::FieldType::I32),
        Ty::I64 => quote!(::lightshadedb::FieldType::I64),
        Ty::U8 => quote!(::lightshadedb::FieldType::U8),
        Ty::U16 => quote!(::lightshadedb::FieldType::U16),
        Ty::U32 => quote!(::lightshadedb::FieldType::U32),
        Ty::U64 => quote!(::lightshadedb::FieldType::U64),
        Ty::F32 => quote!(::lightshadedb::FieldType::F32),
        Ty::F64 => quote!(::lightshadedb::FieldType::F64),
        Ty::Bool => quote!(::lightshadedb::FieldType::Bool),
        Ty::Text => quote!(::lightshadedb::FieldType::Text),
        Ty::Bytes => quote!(::lightshadedb::FieldType::Bytes),
        Ty::Uuid => quote!(::lightshadedb::FieldType::Uuid),
    }
}

pub(crate) fn value_variant(ty: Ty) -> Ident {
    match ty {
        Ty::I8 => format_ident!("I8"),
        Ty::I16 => format_ident!("I16"),
        Ty::I32 => format_ident!("I32"),
        Ty::I64 => format_ident!("I64"),
        Ty::U8 => format_ident!("U8"),
        Ty::U16 => format_ident!("U16"),
        Ty::U32 => format_ident!("U32"),
        Ty::U64 => format_ident!("U64"),
        Ty::F32 => format_ident!("F32"),
        Ty::F64 => format_ident!("F64"),
        Ty::Bool => format_ident!("Bool"),
        Ty::Text => format_ident!("Text"),
        Ty::Bytes => format_ident!("Bytes"),
        Ty::Uuid => format_ident!("Uuid"),
    }
}
