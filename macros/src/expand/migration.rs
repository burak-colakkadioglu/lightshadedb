use super::types::field_type_tokens;
use crate::model::Schema;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

pub(crate) fn migration_support(schema: &Schema) -> TokenStream {
    let compiled_tables = schema.tables.iter().map(|table| {
        let name = &table.db_name;
        let fields = table.fields.iter().map(|field| {
            let field_name = field.ident.to_string();
            let ty = field_type_tokens(field.ir);
            let primary = field.primary_key;
            let has_default = field.default.is_some();
            let nullable = field.nullable;
            quote! {
                ::lightshade::migration::FieldSummary {
                    name: ::std::string::String::from(#field_name),
                    ty: #ty,
                    primary_key: #primary,
                    has_default: #has_default,
                    nullable: #nullable,
                }
            }
        });
        quote! {
            ::lightshade::migration::TableSummary {
                name: ::std::string::String::from(#name),
                fields: ::std::vec![#(#fields,)*],
            }
        }
    });
    let hints = schema.tables.iter().map(|table| {
        let compiled_name = &table.db_name;
        let stored_name = match &table.rename_from {
            Some(name) => quote!(::std::option::Option::Some(::std::string::String::from(#name))),
            None => quote!(::std::option::Option::None),
        };
        let fields = table.fields.iter().map(|field| {
            let compiled_field = field.ident.to_string();
            let stored_field = match &field.rename_from {
                Some(name) => {
                    let name = name.to_string();
                    quote!(::std::option::Option::Some(::std::string::String::from(#name)))
                }
                None => quote!(::std::option::Option::None),
            };
            let convert_from = match &field.convert_from {
                Some((old, _)) => {
                    let ty = field_type_tokens(
                        crate::model::type_to_ir(old).expect("validated conversion type"),
                    );
                    quote!(::std::option::Option::Some(#ty))
                }
                None => quote!(::std::option::Option::None),
            };
            quote! {
                ::lightshade::migration::FieldHint {
                    compiled_name: ::std::string::String::from(#compiled_field),
                    stored_name: #stored_field,
                    convert_from: #convert_from,
                }
            }
        });
        quote! {
            ::lightshade::migration::TableHint {
                compiled_name: ::std::string::String::from(#compiled_name),
                stored_name: #stored_name,
                fields: ::std::vec![#(#fields,)*],
            }
        }
    });
    let conversion_checks = schema.tables.iter().flat_map(|table| {
        table.fields.iter().filter_map(|field| {
            let (old, with) = field.convert_from.as_ref()?;
            let new = &field.ty;
            let name = format_ident!("__ls_convert_check_{}_{}", table.ident, field.ident);
            Some(quote! {
                #[doc(hidden)]
                #[allow(dead_code, non_snake_case)]
                fn #name(value: #old) -> ::lightshade::Result<#new> {
                    #with(value)
                }
            })
        })
    });
    quote! {
        #(#conversion_checks)*

        #[cfg(feature = "embedded")]
        pub fn migration_plan(
            path: impl ::std::convert::AsRef<::std::path::Path>,
        ) -> ::lightshade::Result<::lightshade::MigrationPlan> {
            let path = path.as_ref();
            if !path.exists() {
                return ::std::result::Result::Err(::lightshade::Error::NotFound {
                    path: path.to_path_buf(),
                });
            }
            let (stored_fingerprint, _, stored) =
                __ls_public(__lightshade::Db::__ls_stored_schema(path))?;
            let mut from_tables = ::std::vec::Vec::with_capacity(stored.len());
            for (name, fields) in stored {
                let mut from_fields = ::std::vec::Vec::with_capacity(fields.len());
                for (field, tag, primary_key, nullable) in fields {
                    let ty = ::lightshade::FieldType::from_tag(tag).ok_or(
                        ::lightshade::Error::Corrupt(::lightshade::Corruption::InvalidEncoding),
                    )?;
                    from_fields.push(::lightshade::migration::FieldSummary {
                        name: field,
                        ty,
                        primary_key,
                        has_default: false,
                        nullable,
                    });
                }
                from_tables.push(::lightshade::migration::TableSummary {
                    name,
                    fields: from_fields,
                });
            }
            let from = ::lightshade::migration::SchemaSummary {
                fingerprint: stored_fingerprint,
                tables: from_tables,
            };
            let to = ::lightshade::migration::SchemaSummary {
                fingerprint: 0,
                tables: ::std::vec![#(#compiled_tables,)*],
            };
            let hints = ::std::vec![#(#hints,)*];
            ::lightshade::migration::build_plan(from, to, &hints)
        }

        #[cfg(feature = "embedded")]
        pub fn migrate(
            path: impl ::std::convert::AsRef<::std::path::Path>,
            options: ::lightshade::MigrationOptions,
        ) -> ::lightshade::Result<::lightshade::MigrationReport> {
            let path = path.as_ref();
            if !path.exists() {
                return ::std::result::Result::Err(::lightshade::Error::NotFound {
                    path: path.to_path_buf(),
                });
            }
            let started = ::std::time::Instant::now();
            let plan = migration_plan(path)?;
            options.check(&plan)?;
            let (_, from_lsn, _) = __ls_public(__lightshade::Db::__ls_stored_schema(path))?;
            if plan.is_empty() {
                let (lsn, rows, _, _, _, _) =
                    __ls_public(__lightshade::Db::__ls_admin_verify(path))?;
                return ::std::result::Result::Ok(::lightshade::MigrationReport {
                    plan,
                    from_lsn: lsn,
                    to_lsn: lsn,
                    rows,
                    backup: ::std::option::Option::None,
                    duration: started.elapsed(),
                });
            }
            let backup = if options.backup {
                ::std::option::Option::Some(options.backup_path.clone().unwrap_or_else(|| {
                    ::std::path::PathBuf::from(::std::format!(
                        "{}.pre-migrate-{}.lsdb",
                        path.display(),
                        from_lsn
                    ))
                }))
            } else {
                ::std::option::Option::None
            };
            let (to_lsn, rows) =
                __ls_public(__lightshade::Db::__ls_admin_migrate(path, backup.as_deref()))?;
            ::std::result::Result::Ok(::lightshade::MigrationReport {
                plan,
                from_lsn,
                to_lsn,
                rows,
                backup,
                duration: started.elapsed(),
            })
        }
    }
}
