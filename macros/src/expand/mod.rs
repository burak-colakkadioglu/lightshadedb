mod app;
mod backend;
mod client;
mod db;
mod migration;
mod ops;
mod server;
mod table;
mod types;

use app::app_wrapper;
use client::{async_client_wrapper, client_wrapper};
use db::db_wrapper;
use migration::migration_support;
use ops::typecheck_fn;
use server::server_wrapper;
use table::{projection_select, table_items};

use crate::lower::LoweredSchema;
use crate::model::{strip_lightshadedb_attrs, Schema};
use lightshadedb_codegen::compiler::{
    generate_library_with, GenerateOptions, MigrationField, MigrationTable,
};
use proc_macro2::TokenStream;
use quote::quote;
use std::collections::BTreeMap;
use std::str::FromStr;
use syn::Item;

pub fn expand(schema: Schema) -> syn::Result<TokenStream> {
    let lowered = crate::lower::lower(
        &schema.tables,
        &schema.projections,
        schema
            .operations
            .as_ref()
            .map(|ops| ops.methods.as_slice())
            .unwrap_or(&[]),
    )?;
    let migrations = schema
        .tables
        .iter()
        .map(|table| {
            let fields = table
                .fields
                .iter()
                .map(|field| {
                    (
                        field.ident.to_string(),
                        MigrationField {
                            stored_name: field
                                .rename_from
                                .as_ref()
                                .map(ToString::to_string)
                                .unwrap_or_else(|| field.ident.to_string()),
                            convert: field.convert_from.as_ref().map(|(old, with)| {
                                (
                                    crate::model::type_to_ir(old)
                                        .expect("validated conversion type"),
                                    format!("super::{with}"),
                                )
                            }),
                        },
                    )
                })
                .collect::<BTreeMap<_, _>>();
            (
                table.db_name.clone(),
                MigrationTable {
                    stored_name: table
                        .rename_from
                        .clone()
                        .unwrap_or_else(|| table.db_name.clone()),
                    fields,
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    let source = generate_library_with(
        &lowered.workload,
        &GenerateOptions {
            row_types: lowered
                .tables
                .iter()
                .map(|table| {
                    (
                        table.db_name.clone(),
                        format!("super::{}", table.struct_name),
                    )
                })
                .chain(lowered.projections.iter().map(|projection| {
                    (
                        projection.db_name.clone(),
                        format!("super::{}", projection.struct_name),
                    )
                }))
                .collect::<BTreeMap<_, _>>(),
            migrations,
            emit_row_structs: false,
            emit_inner_attributes: false,
            public_db: true,
        },
    )
    .map_err(|error| {
        syn::Error::new(
            proc_macro2::Span::call_site(),
            format!("Lightshade DB code generation failed: {error}"),
        )
    })?;
    let engine = TokenStream::from_str(&source).map_err(|error| {
        syn::Error::new(
            proc_macro2::Span::call_site(),
            format!("Lightshade DB generated invalid Rust: {error}"),
        )
    })?;

    let mut module = schema.module.clone();
    transform_items(&mut module, &schema);
    let fingerprint = lightshadedb_codegen::compiler::workload_fingerprint(&lowered.workload);
    let generated = expand_items(&schema, &lowered, engine, fingerprint);
    let content = module
        .content
        .as_mut()
        .expect("schema module content was validated");
    content.1.extend(
        syn::parse2::<syn::File>(generated)
            .map_err(|error| {
                syn::Error::new(
                    proc_macro2::Span::call_site(),
                    format!("internal macro output failed to parse: {error}"),
                )
            })?
            .items,
    );
    let tokens = quote!(#module);
    Ok(tokens)
}

fn transform_items(module: &mut syn::ItemMod, schema: &Schema) {
    let table_idents = schema
        .tables
        .iter()
        .map(|table| table.ident.to_string())
        .chain(
            schema
                .projections
                .iter()
                .map(|projection| projection.ident.to_string()),
        )
        .collect::<std::collections::HashSet<_>>();
    let Some((_, items)) = module.content.as_mut() else {
        return;
    };
    for item in items {
        match item {
            Item::Struct(item) if table_idents.contains(&item.ident.to_string()) => {
                strip_lightshadedb_attrs(&mut item.attrs);
                for field in &mut item.fields {
                    strip_lightshadedb_attrs(&mut field.attrs);
                }
                crate::model::ensure_derives(item);
            }
            Item::Trait(item)
                if schema
                    .operations
                    .as_ref()
                    .is_some_and(|ops| ops.ident == item.ident) =>
            {
                for member in &mut item.items {
                    if let syn::TraitItem::Fn(method) = member {
                        strip_lightshadedb_attrs(&mut method.attrs);
                    }
                }
            }
            _ => {}
        }
    }
}

fn expand_items(
    schema: &Schema,
    lowered: &LoweredSchema,
    engine: TokenStream,
    fingerprint: u64,
) -> TokenStream {
    let mut output = TokenStream::new();
    for table in &schema.tables {
        output.extend(table_items(
            &table.ident,
            &table.fields,
            Some((table.primary_key, &table.db_name)),
        ));
    }
    for projection in &schema.projections {
        output.extend(table_items(&projection.ident, &projection.fields, None));
        output.extend(projection_select(&projection.ident, &projection.fields));
    }

    if let Some(operations) = &schema.operations {
        for operation in &operations.methods {
            output.extend(typecheck_fn(operation));
        }
    }

    output.extend(quote! {
        #[cfg(not(any(feature = "embedded", feature = "client")))]
        const _: () = compile_error!(
            "Lightshade DB needs the `embedded` or `client` Cargo feature; forward one from your application crate"
        );

        #[cfg(feature = "embedded")]
        #[doc(hidden)]
        pub mod __lightshadedb {
            #engine
        }

        #[cfg(feature = "embedded")]
        #[doc(hidden)]
        fn __ls_public<T>(result: ::std::io::Result<T>) -> ::lightshadedb::Result<T> {
            result.map_err(__ls_map_io)
        }

        #[cfg(feature = "embedded")]
        #[doc(hidden)]
        fn __ls_map_io(error: ::std::io::Error) -> ::lightshadedb::Error {
            use ::std::io::ErrorKind;
            let message = error.to_string();
            if message.contains("snapshot schema mismatch")
                || message.contains("snapshot workload mismatch")
            {
                return ::lightshadedb::Error::Migration(
                    ::lightshadedb::MigrationError::Required {
                        destructive: false,
                        additions: 0,
                        removals: 0,
                    },
                );
            }
            match error.kind() {
                ErrorKind::PermissionDenied if message.contains("read-only") => {
                    ::lightshadedb::Error::ReadOnly
                }
                ErrorKind::WouldBlock => ::lightshadedb::Error::Locked {
                    path: ::std::path::PathBuf::new(),
                },
                ErrorKind::NotFound => ::lightshadedb::Error::io("opening database", error),
                ErrorKind::InvalidInput => ::lightshadedb::Error::InvalidArgument {
                    name: "database".to_string(),
                    reason: message,
                },
                _ => ::lightshadedb::Error::io("database operation", error),
            }
        }
    });

    output.extend(db_wrapper(schema, lowered));
    output.extend(client_wrapper(schema, lowered, fingerprint));
    output.extend(async_client_wrapper(schema, lowered, fingerprint));
    output.extend(server_wrapper(schema, lowered, fingerprint));
    output.extend(app_wrapper(schema, lowered));
    output.extend(migration_support(schema));
    output.extend(backend_support(schema, lowered));
    output
}

fn backend_support(_schema: &Schema, _lowered: &LoweredSchema) -> TokenStream {
    TokenStream::new()
}
