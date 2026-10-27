use super::types::{field_type_tokens, value_variant};
use crate::lower::LoweredTable;
use crate::model::ModelField;
use lightshadedb_codegen::ir::Ty;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::Ident;

pub(crate) fn table_items(
    ident: &Ident,
    fields: &[ModelField],
    table: Option<(usize, &String)>,
) -> TokenStream {
    let mut handles = TokenStream::new();
    for field in fields {
        let name = field.ident.to_string();
        let ty = &field.ty;
        let field_ty = field_type_tokens(field.ir);
        let field_ident = &field.ident;
        handles.extend(quote! {
            impl #ident {
                #[allow(non_upper_case_globals)]
                pub const #field_ident: ::lightshadedb::Field<#ident, #ty> =
                    ::lightshadedb::Field::new(#name, #field_ty);
            }
        });
    }

    if let Some((_, db_name)) = table {
        handles.extend(quote! {
            #[allow(non_upper_case_globals)]
            #[doc(hidden)]
            pub const #ident: ::lightshadedb::TableHandle =
                ::lightshadedb::TableHandle::new(#db_name);
        });
    }

    let patch_ident = format_ident!("{}Patch", ident);
    let patch_fields = fields
        .iter()
        .filter(|field| !field.primary_key || table.is_none())
        .map(|field| {
            let field_ident = &field.ident;
            let ty = &field.ty;
            quote!(pub #field_ident: ::std::option::Option<#ty>)
        })
        .collect::<Vec<_>>();
    let mut patch = quote! {
        #[derive(Clone, Debug, Default)]
        pub struct #patch_ident {
            #(#patch_fields,)*
        }
    };

    if let Some((primary_key, db_name)) = table {
        let key_field = &fields[primary_key];
        let key_ident = &key_field.ident;
        let key_ty = &key_field.ty;
        let columns = fields.iter().map(|field| {
            let name = field.ident.to_string();
            let ty = field_type_tokens(field.ir);
            let primary = field.primary_key;
            let nullable = field.nullable;
            quote!(::lightshadedb::Column {
                name: #name,
                ty: #ty,
                primary_key: #primary,
                nullable: #nullable,
            })
        });
        let encoders = fields.iter().map(|field| {
            let field_ident = &field.ident;
            let variant = value_variant(field.ir);
            if field.nullable {
                let ty = field_type_tokens(field.ir);
                quote!(match self.#field_ident.clone() {
                    ::std::option::Option::Some(value) => ::lightshadedb::table::Value::#variant(value),
                    ::std::option::Option::None => ::lightshadedb::table::Value::Null(#ty),
                })
            } else {
                quote!(::lightshadedb::table::Value::#variant(self.#field_ident.clone()))
            }
        });
        let decoders = fields.iter().map(|field| {
            let field_ident = &field.ident;
            let field_ident_str = field.ident.to_string();
            let field_ir = field.ir.name();
            let variant = value_variant(field.ir);
            let ty = &field.ty;
            if field.nullable {
                let field_ty = field_type_tokens(field.ir);
                quote! {
                    let #field_ident: #ty = match values.next() {
                        ::std::option::Option::Some(::lightshadedb::table::Value::#variant(value)) => ::std::option::Option::Some(value),
                        ::std::option::Option::Some(::lightshadedb::table::Value::Null(ty)) if ty == #field_ty => ::std::option::Option::None,
                        ::std::option::Option::Some(other) => {
                            return ::std::result::Result::Err(::lightshadedb::Error::InvalidArgument {
                                name: ::std::string::String::from(#field_ident_str),
                                reason: ::std::format!("stored value is {} but the field is {}", other.field_type(), #field_ir),
                            });
                        }
                        ::std::option::Option::None => {
                            return ::std::result::Result::Err(::lightshadedb::Error::InvalidArgument {
                                name: ::std::string::String::from(#field_ident_str),
                                reason: ::std::string::String::from("row has too few values"),
                            });
                        }
                    };
                }
            } else {
                quote! {
                    let #field_ident: #ty = match values.next() {
                        ::std::option::Option::Some(::lightshadedb::table::Value::#variant(value)) => value,
                        ::std::option::Option::Some(other) => {
                            return ::std::result::Result::Err(::lightshadedb::Error::InvalidArgument {
                                name: ::std::string::String::from(#field_ident_str),
                                reason: ::std::format!("stored value is {} but the field is {}", other.field_type(), #field_ir),
                            });
                        }
                        ::std::option::Option::None => {
                            return ::std::result::Result::Err(::lightshadedb::Error::InvalidArgument {
                                name: ::std::string::String::from(#field_ident_str),
                                reason: ::std::string::String::from("row has too few values"),
                            });
                        }
                    };
                }
            }
        });
        let field_idents = fields.iter().map(|field| &field.ident);
        let key_variant = value_variant(key_field.ir);
        let key_from = match key_field.ir {
            Ty::Text => {
                quote!(::lightshadedb::table::Value::Text(value) => ::std::result::Result::Ok(value))
            }
            Ty::Bytes => {
                quote!(::lightshadedb::table::Value::Bytes(value) => ::std::result::Result::Ok(value))
            }
            Ty::Uuid => {
                quote!(::lightshadedb::table::Value::Uuid(value) => ::std::result::Result::Ok(value))
            }
            Ty::Bool => {
                quote!(::lightshadedb::table::Value::Bool(value) => ::std::result::Result::Ok(value))
            }
            Ty::I8 => {
                quote!(::lightshadedb::table::Value::I8(value) => ::std::result::Result::Ok(value))
            }
            Ty::I16 => {
                quote!(::lightshadedb::table::Value::I16(value) => ::std::result::Result::Ok(value))
            }
            Ty::I32 => {
                quote!(::lightshadedb::table::Value::I32(value) => ::std::result::Result::Ok(value))
            }
            Ty::I64 => {
                quote!(::lightshadedb::table::Value::I64(value) => ::std::result::Result::Ok(value))
            }
            Ty::U8 => {
                quote!(::lightshadedb::table::Value::U8(value) => ::std::result::Result::Ok(value))
            }
            Ty::U16 => {
                quote!(::lightshadedb::table::Value::U16(value) => ::std::result::Result::Ok(value))
            }
            Ty::U32 => {
                quote!(::lightshadedb::table::Value::U32(value) => ::std::result::Result::Ok(value))
            }
            Ty::U64 => {
                quote!(::lightshadedb::table::Value::U64(value) => ::std::result::Result::Ok(value))
            }
            Ty::F32 => {
                quote!(::lightshadedb::table::Value::F32(value) => ::std::result::Result::Ok(value))
            }
            Ty::F64 => {
                quote!(::lightshadedb::table::Value::F64(value) => ::std::result::Result::Ok(value))
            }
        };
        let patch_apply = fields
            .iter()
            .filter(|field| !field.primary_key)
            .map(|field| {
                let field_ident = &field.ident;
                quote! {
                    if let ::std::option::Option::Some(value) = patch.#field_ident.clone() {
                        self.#field_ident = value;
                    }
                }
            });
        if table.is_some() {
            handles.extend(quote! {
                impl #ident {
                    pub fn join_on(predicate: ::lightshadedb::Predicate) -> ::lightshadedb::Join {
                        let _ = predicate;
                        ::lightshadedb::Join::new()
                    }
                }
            });
        }
        patch.extend(quote! {
            impl ::lightshadedb::Table for #ident {
                type Key = #key_ty;
                type Patch = #patch_ident;

                const NAME: &'static str = #db_name;
                const COLUMNS: &'static [::lightshadedb::Column] = &[
                    #(#columns,)*
                ];

                fn key(&self) -> &Self::Key {
                    &self.#key_ident
                }

                fn to_values(&self) -> ::std::vec::Vec<::lightshadedb::table::Value> {
                    ::std::vec![#(#encoders,)*]
                }

                fn from_values(values: ::std::vec::Vec<::lightshadedb::table::Value>) -> ::lightshadedb::Result<Self> {
                    let mut values = values.into_iter();
                    #(#decoders)*
                    if values.next().is_some() {
                        return ::std::result::Result::Err(::lightshadedb::Error::InvalidArgument {
                            name: ::std::string::String::from("row"),
                            reason: ::std::string::String::from("row has too many values"),
                        });
                    }
                    ::std::result::Result::Ok(Self { #(#field_idents),* })
                }

                #[allow(clippy::ptr_arg)]
                fn key_to_value(key: &Self::Key) -> ::lightshadedb::table::Value {
                    ::lightshadedb::table::Value::#key_variant(key.clone())
                }

                fn key_from_value(value: ::lightshadedb::table::Value) -> ::lightshadedb::Result<Self::Key> {
                    match value {
                        #key_from,
                        other => ::std::result::Result::Err(::lightshadedb::Error::InvalidArgument {
                            name: ::std::string::String::from("primary key"),
                            reason: ::std::format!("stored key is {}", other.field_type()),
                        }),
                    }
                }

                fn apply_patch(&mut self, patch: &Self::Patch) {
                    #(#patch_apply)*
                }
            }
        });
    } else {
        let fields_tokens = fields.iter().map(|field| {
            let name = field.ident.to_string();
            let ty = field_type_tokens(field.ir);
            let primary = field.primary_key;
            let nullable = field.nullable;
            quote!(::lightshadedb::Column {
                name: #name,
                ty: #ty,
                primary_key: #primary,
                nullable: #nullable,
            })
        });
        let decode = fields.iter().map(|field| {
            let field_ident = &field.ident;
            let field_ident_str = field.ident.to_string();
            let field_ir = field.ir.name();
            let variant = value_variant(field.ir);
            let ty = &field.ty;
            if field.nullable {
                let field_ty = field_type_tokens(field.ir);
                quote! {
                    let #field_ident: #ty = match values.next() {
                        ::std::option::Option::Some(::lightshadedb::table::Value::#variant(value)) => ::std::option::Option::Some(value),
                        ::std::option::Option::Some(::lightshadedb::table::Value::Null(ty)) if ty == #field_ty => ::std::option::Option::None,
                        ::std::option::Option::Some(other) => {
                            return ::std::result::Result::Err(::lightshadedb::Error::InvalidArgument {
                                name: ::std::string::String::from(#field_ident_str),
                                reason: ::std::format!("projected value is {} but the field is {}", other.field_type(), #field_ir),
                            });
                        }
                        ::std::option::Option::None => {
                            return ::std::result::Result::Err(::lightshadedb::Error::InvalidArgument {
                                name: ::std::string::String::from(#field_ident_str),
                                reason: ::std::string::String::from("projection returned too few values"),
                            });
                        }
                    };
                }
            } else {
                quote! {
                    let #field_ident: #ty = match values.next() {
                        ::std::option::Option::Some(::lightshadedb::table::Value::#variant(value)) => value,
                        ::std::option::Option::Some(other) => {
                            return ::std::result::Result::Err(::lightshadedb::Error::InvalidArgument {
                                name: ::std::string::String::from(#field_ident_str),
                                reason: ::std::format!("projected value is {} but the field is {}", other.field_type(), #field_ir),
                            });
                        }
                        ::std::option::Option::None => {
                            return ::std::result::Result::Err(::lightshadedb::Error::InvalidArgument {
                                name: ::std::string::String::from(#field_ident_str),
                                reason: ::std::string::String::from("projection returned too few values"),
                            });
                        }
                    };
                }
            }
        });
        let field_idents = fields.iter().map(|field| &field.ident);
        let projection_encoders = fields.iter().map(|field| {
            let field_ident = &field.ident;
            let variant = value_variant(field.ir);
            if field.nullable {
                let ty = field_type_tokens(field.ir);
                quote!(match self.#field_ident.clone() {
                    ::std::option::Option::Some(value) => ::lightshadedb::table::Value::#variant(value),
                    ::std::option::Option::None => ::lightshadedb::table::Value::Null(#ty),
                })
            } else {
                quote!(::lightshadedb::table::Value::#variant(self.#field_ident.clone()))
            }
        });
        patch.extend(quote! {
            impl ::lightshadedb::Projection for #ident {
                const FIELDS: &'static [::lightshadedb::Column] = &[
                    #(#fields_tokens,)*
                ];

                fn from_values(values: ::std::vec::Vec<::lightshadedb::table::Value>) -> ::lightshadedb::Result<Self> {
                    let mut values = values.into_iter();
                    #(#decode)*
                    if values.next().is_some() {
                        return ::std::result::Result::Err(::lightshadedb::Error::InvalidArgument {
                            name: ::std::string::String::from("projection"),
                            reason: ::std::string::String::from("projection returned too many values"),
                        });
                    }
                    ::std::result::Result::Ok(Self { #(#field_idents),* })
                }

                fn to_values(&self) -> ::std::vec::Vec<::lightshadedb::table::Value> {
                    ::std::vec![#(#projection_encoders,)*]
                }
            }
        });
    }

    quote!(#handles #patch)
}

pub(crate) fn projection_select(ident: &Ident, fields: &[ModelField]) -> TokenStream {
    let select_ident = format_ident!("{}Select", ident);
    let methods = fields.iter().map(|field| {
        let method = &field.ident;
        let ty = &field.ty;
        quote! {
            pub fn #method(self, value: impl ::std::convert::Into<::lightshadedb::SelectValue<#ty>>) -> Self {
                let _ = value;
                self
            }
        }
    });
    quote! {
        impl #ident {
            pub fn select() -> #select_ident {
                #select_ident
            }
        }

        #[derive(Clone, Copy, Debug, Default)]
        pub struct #select_ident;

        impl #select_ident {
            #(#methods)*
        }
    }
}

pub(crate) fn table_ops_impl(table: &LoweredTable, ident: &Ident) -> TokenStream {
    let insert = format_ident!("{}", table.crud.insert);
    let get = format_ident!("{}", table.crud.get);
    let contains = format_ident!("{}", table.crud.contains);
    let scan = format_ident!("{}", table.crud.scan);
    let count = format_ident!("{}", table.crud.count);
    let update = format_ident!("{}", table.crud.update);
    let delete = format_ident!("{}", table.crud.delete);
    let fields = &table.columns;
    let field_args = fields.iter().map(|column| format_ident!("{}", column.name));
    let update_args = fields.iter().map(|column| {
        let name = format_ident!("{}", column.name);
        quote!(row.#name)
    });
    let key_ident = format_ident!("{}", table.primary_key);
    let key_arg = quote!(key.clone());
    let bulk_insert = format_ident!("__ls_bulk_insert_{}", table.db_name);
    let patch_fields = fields
        .iter()
        .filter(|column| !column.primary_key)
        .map(|column| {
            let name = format_ident!("{}", column.name);
            quote! {
                if let ::std::option::Option::Some(value) = patch.#name {
                    row.#name = value;
                }
            }
        });
    let db_name = &table.db_name;
    quote! {
        #[cfg(feature = "embedded")]
        impl ::lightshadedb::table::TableOps<Db> for #ident {
            fn insert(store: &mut Db, row: Self) -> ::lightshadedb::Result<()> {
                let inserted = __ls_public(store.inner.#insert(#(row.#field_args),*))?;
                if inserted {
                    ::std::result::Result::Ok(())
                } else {
                    ::std::result::Result::Err(::lightshadedb::Error::DuplicateKey {
                        table: ::std::string::String::from(#db_name),
                    })
                }
            }

            fn insert_many(store: &mut Db, rows: ::std::vec::Vec<Self>) -> ::lightshadedb::Result<()> {
                let own = !store.in_transaction();
                if own {
                    store.begin()?;
                }
                let expected = rows.len() as u64;
                let outcome = __ls_public(store.inner.#bulk_insert(rows));
                match outcome {
                    ::std::result::Result::Ok(inserted) if inserted == expected => {
                        if own { store.commit() } else { ::std::result::Result::Ok(()) }
                    }
                    ::std::result::Result::Ok(_) => {
                        if own {
                            let _ = store.rollback();
                        }
                        ::std::result::Result::Err(::lightshadedb::Error::DuplicateKey {
                            table: ::std::string::String::from(#db_name),
                        })
                    }
                    ::std::result::Result::Err(error) => {
                        if own {
                            let _ = store.rollback();
                        }
                        ::std::result::Result::Err(error)
                    }
                }
            }

            fn upsert(store: &mut Db, row: Self) -> ::lightshadedb::Result<bool> {
                let own = !store.in_transaction();
                if own {
                    store.begin()?;
                }
                let outcome = (|| {
                    let key = row.#key_ident.clone();
                    if <Self as ::lightshadedb::table::TableOps<Db>>::contains(store, &key)? {
                        <Self as ::lightshadedb::table::TableOps<Db>>::update(store, row)?;
                        ::std::result::Result::Ok(false)
                    } else {
                        <Self as ::lightshadedb::table::TableOps<Db>>::insert(store, row)?;
                        ::std::result::Result::Ok(true)
                    }
                })();
                match outcome {
                    ::std::result::Result::Ok(value) => {
                        if own { store.commit()?; }
                        ::std::result::Result::Ok(value)
                    }
                    ::std::result::Result::Err(error) => {
                        if own { let _ = store.rollback(); }
                        ::std::result::Result::Err(error)
                    }
                }
            }

            fn upsert_many(store: &mut Db, rows: ::std::vec::Vec<Self>) -> ::lightshadedb::Result<u64> {
                let own = !store.in_transaction();
                if own {
                    store.begin()?;
                }
                let mut inserted = 0u64;
                for row in rows {
                    match <Self as ::lightshadedb::table::TableOps<Db>>::upsert(store, row) {
                        ::std::result::Result::Ok(true) => inserted += 1,
                        ::std::result::Result::Ok(false) => {}
                        ::std::result::Result::Err(error) => {
                            if own { let _ = store.rollback(); }
                            return ::std::result::Result::Err(error);
                        }
                    }
                }
                if own { store.commit()?; }
                ::std::result::Result::Ok(inserted)
            }

            fn get(store: &Db, key: Self::Key) -> ::lightshadedb::Result<::std::option::Option<Self>> {
                let mut rows = store.inner.#get(key);
                ::std::result::Result::Ok(rows.pop())
            }

            #[allow(clippy::ptr_arg)]
            fn contains(store: &Db, key: &Self::Key) -> ::lightshadedb::Result<bool> {
                ::std::result::Result::Ok(store.inner.#contains(key.clone()) > 0)
            }

            fn update(store: &mut Db, row: Self) -> ::lightshadedb::Result<bool> {
                ::std::result::Result::Ok(__ls_public(store.inner.#update(#(#update_args),*))? > 0)
            }

            fn patch(store: &mut Db, key: Self::Key, patch: Self::Patch) -> ::lightshadedb::Result<bool> {
                let ::std::option::Option::Some(mut row) =
                    <Self as ::lightshadedb::table::TableOps<Db>>::get(store, key)?
                else {
                    return ::std::result::Result::Ok(false);
                };
                #(#patch_fields)*
                <Self as ::lightshadedb::table::TableOps<Db>>::update(store, row)
            }

            fn delete(store: &mut Db, key: Self::Key) -> ::lightshadedb::Result<bool> {
                ::std::result::Result::Ok(__ls_public(store.inner.#delete(#key_arg))? > 0)
            }

            fn count(store: &Db) -> ::lightshadedb::Result<u64> {
                ::std::result::Result::Ok(store.inner.#count() as u64)
            }

            fn scan(store: &Db) -> ::lightshadedb::Result<::std::vec::Vec<Self>> {
                ::std::result::Result::Ok(store.inner.#scan())
            }
        }
    }
}
