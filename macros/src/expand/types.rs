use lightshade_codegen::ir::Ty;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::Ident;

pub(crate) fn field_type_tokens(ty: Ty) -> TokenStream {
    match ty {
        Ty::I8 => quote!(::lightshade::FieldType::I8),
        Ty::I16 => quote!(::lightshade::FieldType::I16),
        Ty::I32 => quote!(::lightshade::FieldType::I32),
        Ty::I64 => quote!(::lightshade::FieldType::I64),
        Ty::U8 => quote!(::lightshade::FieldType::U8),
        Ty::U16 => quote!(::lightshade::FieldType::U16),
        Ty::U32 => quote!(::lightshade::FieldType::U32),
        Ty::U64 => quote!(::lightshade::FieldType::U64),
        Ty::F32 => quote!(::lightshade::FieldType::F32),
        Ty::F64 => quote!(::lightshade::FieldType::F64),
        Ty::Bool => quote!(::lightshade::FieldType::Bool),
        Ty::Text => quote!(::lightshade::FieldType::Text),
        Ty::Bytes => quote!(::lightshade::FieldType::Bytes),
        Ty::Uuid => quote!(::lightshade::FieldType::Uuid),
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
