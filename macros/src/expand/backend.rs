use crate::lower::LoweredSchema;
use crate::model::Schema;
use proc_macro2::TokenStream;
use quote::quote;

pub(crate) fn backend_impl_for(schema: &Schema, lowered: &LoweredSchema) -> TokenStream {
    let arms = |method: &str| {
        lowered
            .tables
            .iter()
            .zip(schema.tables.iter())
            .map(|(table, model)| {
                let db_name = &table.db_name;
                let ident = &model.ident;
                let call = match method {
                    "insert" => quote! {
                        {
                            let row = <#ident as ::lightshade::Table>::from_values(values)?;
                            <#ident as ::lightshade::table::TableOps<Db>>::insert(self, row)
                        }
                    },
                    "insert_many" => quote! {
                        {
                            let rows = rows
                                .into_iter()
                                .map(<#ident as ::lightshade::Table>::from_values)
                                .collect::<::lightshade::Result<::std::vec::Vec<_>>>()?;
                            <#ident as ::lightshade::table::TableOps<Db>>::insert_many(self, rows)
                        }
                    },
                    "upsert" => quote! {
                        {
                            let row = <#ident as ::lightshade::Table>::from_values(values)?;
                            <#ident as ::lightshade::table::TableOps<Db>>::upsert(self, row)
                        }
                    },
                    "upsert_many" => quote! {
                        {
                            let rows = rows
                                .into_iter()
                                .map(<#ident as ::lightshade::Table>::from_values)
                                .collect::<::lightshade::Result<::std::vec::Vec<_>>>()?;
                            <#ident as ::lightshade::table::TableOps<Db>>::upsert_many(self, rows)
                        }
                    },
                    "get" => quote! {
                        {
                            let key = <#ident as ::lightshade::Table>::key_from_value(key)?;
                            match <#ident as ::lightshade::table::TableOps<Db>>::get(self, key)? {
                                ::std::option::Option::Some(row) => {
                                    ::std::result::Result::Ok(::std::option::Option::Some(
                                        <#ident as ::lightshade::Table>::to_values(&row),
                                    ))
                                }
                                ::std::option::Option::None => ::std::result::Result::Ok(::std::option::Option::None),
                            }
                        }
                    },
                    "contains" => quote! {
                        {
                            let key = <#ident as ::lightshade::Table>::key_from_value(key)?;
                            <#ident as ::lightshade::table::TableOps<Db>>::contains(self, &key)
                        }
                    },
                    "update" => quote! {
                        {
                            let row = <#ident as ::lightshade::Table>::from_values(values)?;
                            <#ident as ::lightshade::table::TableOps<Db>>::update(self, row)
                        }
                    },
                    "delete" => quote! {
                        {
                            let key = <#ident as ::lightshade::Table>::key_from_value(key)?;
                            <#ident as ::lightshade::table::TableOps<Db>>::delete(self, key)
                        }
                    },
                    "count" => quote!(<#ident as ::lightshade::table::TableOps<Db>>::count(self)),
                    "scan" => quote! {
                        {
                            let rows = <#ident as ::lightshade::table::TableOps<Db>>::scan(self)?;
                            ::std::result::Result::Ok(rows
                                .iter()
                                .map(<#ident as ::lightshade::Table>::to_values)
                                .collect())
                        }
                    },
                    _ => unreachable!(),
                };
                quote!(#db_name => #call,)
            })
            .collect::<Vec<_>>()
    };
    let insert = arms("insert");
    let insert_many = arms("insert_many");
    let upsert = arms("upsert");
    let upsert_many = arms("upsert_many");
    let get = arms("get");
    let contains = arms("contains");
    let update = arms("update");
    let delete = arms("delete");
    let count = arms("count");
    let scan = arms("scan");
    quote! {
        #[cfg(feature = "embedded")]
        impl ::lightshade::Backend for Db {
            fn __insert(&mut self, table: &'static str, values: ::std::vec::Vec<::lightshade::table::Value>) -> ::lightshade::Result<()> {
                match table {
                    #(#insert)*
                    other => ::std::result::Result::Err(::lightshade::Error::Schema(
                        ::lightshade::SchemaError::UnknownTable { name: other.to_string() },
                    )),
                }
            }
            fn __insert_many(&mut self, table: &'static str, rows: ::std::vec::Vec<::std::vec::Vec<::lightshade::table::Value>>) -> ::lightshade::Result<()> {
                match table {
                    #(#insert_many)*
                    other => ::std::result::Result::Err(::lightshade::Error::Schema(
                        ::lightshade::SchemaError::UnknownTable { name: other.to_string() },
                    )),
                }
            }
            fn __upsert(&mut self, table: &'static str, values: ::std::vec::Vec<::lightshade::table::Value>) -> ::lightshade::Result<bool> {
                match table {
                    #(#upsert)*
                    other => ::std::result::Result::Err(::lightshade::Error::Schema(
                        ::lightshade::SchemaError::UnknownTable { name: other.to_string() },
                    )),
                }
            }
            fn __upsert_many(&mut self, table: &'static str, rows: ::std::vec::Vec<::std::vec::Vec<::lightshade::table::Value>>) -> ::lightshade::Result<u64> {
                match table {
                    #(#upsert_many)*
                    other => ::std::result::Result::Err(::lightshade::Error::Schema(
                        ::lightshade::SchemaError::UnknownTable { name: other.to_string() },
                    )),
                }
            }
            fn __get(&self, table: &'static str, key: ::lightshade::table::Value) -> ::lightshade::Result<::std::option::Option<::std::vec::Vec<::lightshade::table::Value>>> {
                match table {
                    #(#get)*
                    other => ::std::result::Result::Err(::lightshade::Error::Schema(
                        ::lightshade::SchemaError::UnknownTable { name: other.to_string() },
                    )),
                }
            }
            fn __contains(&self, table: &'static str, key: ::lightshade::table::Value) -> ::lightshade::Result<bool> {
                match table {
                    #(#contains)*
                    other => ::std::result::Result::Err(::lightshade::Error::Schema(
                        ::lightshade::SchemaError::UnknownTable { name: other.to_string() },
                    )),
                }
            }
            fn __update(&mut self, table: &'static str, values: ::std::vec::Vec<::lightshade::table::Value>) -> ::lightshade::Result<bool> {
                match table {
                    #(#update)*
                    other => ::std::result::Result::Err(::lightshade::Error::Schema(
                        ::lightshade::SchemaError::UnknownTable { name: other.to_string() },
                    )),
                }
            }
            fn __delete(&mut self, table: &'static str, key: ::lightshade::table::Value) -> ::lightshade::Result<bool> {
                match table {
                    #(#delete)*
                    other => ::std::result::Result::Err(::lightshade::Error::Schema(
                        ::lightshade::SchemaError::UnknownTable { name: other.to_string() },
                    )),
                }
            }
            fn __count(&self, table: &'static str) -> ::lightshade::Result<u64> {
                match table {
                    #(#count)*
                    other => ::std::result::Result::Err(::lightshade::Error::Schema(
                        ::lightshade::SchemaError::UnknownTable { name: other.to_string() },
                    )),
                }
            }
            fn __scan(&self, table: &'static str) -> ::lightshade::Result<::std::vec::Vec<::std::vec::Vec<::lightshade::table::Value>>> {
                match table {
                    #(#scan)*
                    other => ::std::result::Result::Err(::lightshade::Error::Schema(
                        ::lightshade::SchemaError::UnknownTable { name: other.to_string() },
                    )),
                }
            }
            fn begin(&mut self) -> ::lightshade::Result<()> { Db::begin(self) }
            fn commit(&mut self) -> ::lightshade::Result<()> { Db::commit(self) }
            fn rollback(&mut self) -> ::lightshade::Result<()> { Db::rollback(self) }
            fn in_transaction(&self) -> bool { Db::in_transaction(self) }
            fn transaction<T, F>(&mut self, f: F) -> ::lightshade::Result<T>
            where F: ::std::ops::FnOnce(&mut Self) -> ::lightshade::Result<T>, Self: ::std::marker::Sized {
                Db::transaction(self, f)
            }
            fn close(self) -> ::lightshade::Result<()> { Db::close(self) }
        }
    }
}
