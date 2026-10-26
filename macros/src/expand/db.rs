use crate::lower::LoweredSchema;
use crate::model::Schema;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use super::backend::backend_impl_for;
use super::ops::{operation_impls, operation_trait_impls};
use super::table::table_ops_impl;
use super::types::field_type_tokens;
pub(crate) fn db_wrapper(schema: &Schema, lowered: &LoweredSchema) -> TokenStream {
    let table_ops = lowered
        .tables
        .iter()
        .zip(schema.tables.iter())
        .map(|(lowered_table, table)| table_ops_impl(lowered_table, &table.ident));
    let backend_impl = backend_impl_for(schema, lowered);
    let operation_impls = operation_impls(schema, &format_ident!("Db"), true);
    let operation_trait = operation_trait_impls(schema, &format_ident!("Db"));
    let dataplane_tables: Vec<&syn::Ident> =
        schema.tables.iter().map(|table| &table.ident).collect();
    let dataplane_names: Vec<&String> = schema.tables.iter().map(|table| &table.db_name).collect();
    let verify_tables = schema.tables.iter().enumerate().map(|(index, table)| {
        let name = &table.db_name;
        quote! {
            ::lightshade::admin::TableReport {
                name: ::std::string::String::from(#name),
                rows: counts.get(#index).copied().unwrap_or(0),
            }
        }
    });
    let inspect_tables = schema.tables.iter().enumerate().map(|(index, table)| {
        let name = &table.db_name;
        let fields = table.fields.iter().map(|field| {
            let field_name = field.ident.to_string();
            let ty = field_type_tokens(field.ir);
            let primary = field.primary_key;
            let has_default = field.default.is_some();
            let nullable = field.nullable;
            quote! {
                ::lightshade::admin::FieldInspect {
                    name: ::std::string::String::from(#field_name),
                    ty: #ty,
                    primary_key: #primary,
                    has_default: #has_default,
                    nullable: #nullable,
                }
            }
        });
        quote! {
            ::lightshade::admin::TableInspect {
                name: ::std::string::String::from(#name),
                rows: counts.get(#index).copied().unwrap_or(0),
                fields: ::std::vec![#(#fields,)*],
            }
        }
    });
    quote! {
        #[cfg(feature = "embedded")]
        pub struct Db {
            inner: __lightshade::Db,
            path: ::std::path::PathBuf,
        }

        #[cfg(feature = "embedded")]
        impl Db {
            pub fn open(path: impl ::std::convert::AsRef<::std::path::Path>) -> ::lightshade::Result<Self> {
                let path = path.as_ref();
                let inner = match __lightshade::Db::open(path) {
                    ::std::result::Result::Ok(inner) => inner,
                    ::std::result::Result::Err(error)
                        if error.to_string().contains("snapshot schema mismatch")
                        || error.to_string().contains("snapshot workload mismatch") =>
                    {
                        let plan = migration_plan(path)?;
                        if plan.is_empty() {
                            __ls_public(__lightshade::Db::open_migrated(path))?
                        } else {
                            return ::std::result::Result::Err(::lightshade::Error::Migration(
                                ::lightshade::MigrationError::Required {
                                    destructive: plan.destructive,
                                    additions: plan.additions,
                                    removals: plan.removals,
                                },
                            ));
                        }
                    }
                    ::std::result::Result::Err(error) => return ::std::result::Result::Err(__ls_map_io(error)),
                };
                Ok(Self { inner, path: path.to_path_buf() })
            }

            pub fn open_with(
                path: impl ::std::convert::AsRef<::std::path::Path>,
                options: ::lightshade::OpenOptions,
            ) -> ::lightshade::Result<Self> {
                let path = path.as_ref();
                if !options.create && !path.exists() {
                    return ::std::result::Result::Err(::lightshade::Error::NotFound {
                        path: path.to_path_buf(),
                    });
                }
                if options.read_only {
                    return Self::open_read_only(path);
                }
                if options.migrate {
                    return Self::open_migrated(path);
                }
                let inner = __ls_public(__lightshade::Db::open(path))?;
                Ok(Self { inner, path: path.to_path_buf() })
            }

            pub fn open_read_only(
                path: impl ::std::convert::AsRef<::std::path::Path>,
            ) -> ::lightshade::Result<Self> {
                let path = path.as_ref();
                if !path.exists() {
                    return ::std::result::Result::Err(::lightshade::Error::NotFound {
                        path: path.to_path_buf(),
                    });
                }
                let inner = __ls_public(__lightshade::Db::open_read_only(path))?;
                Ok(Self { inner, path: path.to_path_buf() })
            }

            pub fn open_migrated(
                path: impl ::std::convert::AsRef<::std::path::Path>,
            ) -> ::lightshade::Result<Self> {
                Self::open_migrated_with(path, ::lightshade::MigrationOptions::default())
            }

            pub fn open_migrated_with(
                path: impl ::std::convert::AsRef<::std::path::Path>,
                options: ::lightshade::MigrationOptions,
            ) -> ::lightshade::Result<Self> {
                let path = path.as_ref();
                migrate(path, options)?;
                Self::open(path)
            }

            pub fn open_in_memory() -> ::lightshade::Result<Self> {
                let inner = __ls_public(__lightshade::Db::open_in_memory())?;
                Ok(Self { inner, path: ::std::path::PathBuf::new() })
            }

            pub fn recover(
                path: impl ::std::convert::AsRef<::std::path::Path>,
                options: ::lightshade::RecoverOptions,
            ) -> ::lightshade::Result<Self> {
                let path = path.as_ref();
                let inner = __ls_public(__lightshade::Db::recover(path, options.discard_wal))?;
                Ok(Self { inner, path: path.to_path_buf() })
            }

            pub fn path(&self) -> &::std::path::Path {
                &self.path
            }

            pub fn lsn(&self) -> u64 {
                self.inner.lsn()
            }

            pub fn is_read_only(&self) -> bool {
                self.inner.is_read_only()
            }

            pub fn in_transaction(&self) -> bool {
                self.inner.in_transaction()
            }

            pub fn flush(&mut self) -> ::lightshade::Result<()> {
                __ls_public(self.inner.flush())
            }

            pub fn checkpoint(&mut self) -> ::lightshade::Result<()> {
                __ls_public(self.inner.checkpoint())
            }

            pub fn verify(
                path: impl ::std::convert::AsRef<::std::path::Path>,
                options: ::lightshade::admin::VerifyOptions,
            ) -> ::lightshade::Result<::lightshade::admin::VerifyReport> {
                let path = path.as_ref();
                if !path.exists() {
                    return ::std::result::Result::Err(::lightshade::Error::NotFound {
                        path: path.to_path_buf(),
                    });
                }
                let started = ::std::time::Instant::now();
                let (lsn, rows, snapshot_bytes, wal_bytes, schema_fingerprint, counts) =
                    __ls_public(__lightshade::Db::__ls_admin_verify(path))?;
                Ok(::lightshade::admin::VerifyReport {
                    ok: true,
                    lsn,
                    schema_fingerprint,
                    tables: ::std::vec![#(#verify_tables,)*],
                    rows,
                    snapshot_bytes,
                    wal_bytes,
                    wal_records: 0,
                    torn_tail: !options.tolerate_torn_tail,
                    duration: started.elapsed(),
                })
            }

            pub fn inspect(
                path: impl ::std::convert::AsRef<::std::path::Path>,
            ) -> ::lightshade::Result<::lightshade::admin::InspectReport> {
                let path = path.as_ref();
                if !path.exists() {
                    return ::std::result::Result::Err(::lightshade::Error::NotFound {
                        path: path.to_path_buf(),
                    });
                }
                let (format_version, wal_version, schema_fingerprint, workload, lsn, rows, snapshot_bytes, wal_bytes, counts) =
                    __ls_public(__lightshade::Db::__ls_admin_inspect(path))?;
                Ok(::lightshade::admin::InspectReport {
                    path: path.to_path_buf(),
                    format_version,
                    wal_version,
                    schema_fingerprint,
                    workload_fingerprint: workload,
                    lsn,
                    rows,
                    snapshot_bytes,
                    wal_bytes,
                    tables: ::std::vec![#(#inspect_tables,)*],
                })
            }

            pub fn backup(
                source: impl ::std::convert::AsRef<::std::path::Path>,
                destination: impl ::std::convert::AsRef<::std::path::Path>,
                options: ::lightshade::admin::BackupOptions,
            ) -> ::lightshade::Result<::lightshade::admin::BackupReport> {
                let source = source.as_ref();
                let destination = destination.as_ref();
                if !source.exists() {
                    return ::std::result::Result::Err(::lightshade::Error::NotFound {
                        path: source.to_path_buf(),
                    });
                }
                let (lsn, rows, bytes) = __ls_public(
                    __lightshade::Db::__ls_admin_backup(source, destination, options.overwrite),
                )?;
                Ok(::lightshade::admin::BackupReport {
                    lsn,
                    rows,
                    bytes,
                    destination: destination.to_path_buf(),
                })
            }

            pub fn compact(
                path: impl ::std::convert::AsRef<::std::path::Path>,
            ) -> ::lightshade::Result<::lightshade::admin::CompactReport> {
                let path = path.as_ref();
                if !path.exists() {
                    return ::std::result::Result::Err(::lightshade::Error::NotFound {
                        path: path.to_path_buf(),
                    });
                }
                let (lsn, rows, snapshot_before, snapshot_after, wal_before, wal_after) =
                    __ls_public(__lightshade::Db::__ls_admin_compact(path))?;
                Ok(::lightshade::admin::CompactReport {
                    lsn,
                    rows,
                    snapshot_before,
                    snapshot_after,
                    wal_before,
                    wal_after,
                })
            }

            pub fn recover_report(
                path: impl ::std::convert::AsRef<::std::path::Path>,
                discard_wal: bool,
            ) -> ::lightshade::Result<::lightshade::admin::RecoverReport> {
                let path = path.as_ref();
                if !path.exists() {
                    return ::std::result::Result::Err(::lightshade::Error::NotFound {
                        path: path.to_path_buf(),
                    });
                }
                let (lsn, rows) =
                    __ls_public(__lightshade::Db::__ls_admin_recover(path, discard_wal))?;
                Ok(::lightshade::admin::RecoverReport {
                    lsn,
                    wal_records: 0,
                    discarded_wal: discard_wal,
                    rows,
                })
            }

            pub fn close(self) -> ::lightshade::Result<()> {
                __ls_public(self.inner.close())
            }

            pub fn begin(&mut self) -> ::lightshade::Result<()> {
                __ls_public(self.inner.begin())
            }

            pub fn commit(&mut self) -> ::lightshade::Result<()> {
                __ls_public(self.inner.commit())
            }

            pub fn rollback(&mut self) -> ::lightshade::Result<()> {
                __ls_public(self.inner.rollback())
            }

            #[cfg(feature = "host")]
            fn __ls_current_lsn(&self) -> u64 {
                self.inner.__ls_current_lsn()
            }

            #[cfg(feature = "host")]
            fn __ls_wal_sync_handle(
                &self,
            ) -> ::std::option::Option<
                ::std::sync::Arc<__lightshade::__LsWalSync>,
            > {
                self.inner.__ls_wal_sync_handle()
            }

            #[cfg(feature = "host")]
            fn __ls_start_group_commit(
                &self,
                shutdown: &::std::sync::Arc<::std::sync::atomic::AtomicBool>,
            ) -> ::std::option::Option<::std::thread::JoinHandle<()>> {
                self.inner.__ls_start_group_commit(shutdown)
            }

            pub fn transaction<T>(
                &mut self,
                f: impl ::std::ops::FnOnce(&mut Db) -> ::lightshade::Result<T>,
            ) -> ::lightshade::Result<T> {
                self.begin()?;
                let outcome = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| f(self)));
                match outcome {
                    ::std::result::Result::Ok(::std::result::Result::Ok(value)) => {
                        self.commit()?;
                        ::std::result::Result::Ok(value)
                    }
                    ::std::result::Result::Ok(::std::result::Result::Err(error)) => {
                        let _ = self.rollback();
                        ::std::result::Result::Err(error)
                    }
                    ::std::result::Result::Err(payload) => {
                        let _ = self.rollback();
                        ::std::panic::resume_unwind(payload)
                    }
                }
            }

            pub fn insert<T: ::lightshade::table::TableOps<Db>>(&mut self, row: T) -> ::lightshade::Result<()> {
                T::insert(self, row)
            }

            pub fn insert_many<T: ::lightshade::table::TableOps<Db>>(
                &mut self,
                rows: impl ::std::iter::IntoIterator<Item = T>,
            ) -> ::lightshade::Result<()> {
                T::insert_many(self, rows.into_iter().collect())
            }

            pub fn upsert<T: ::lightshade::table::TableOps<Db>>(&mut self, row: T) -> ::lightshade::Result<bool> {
                T::upsert(self, row)
            }

            pub fn upsert_many<T: ::lightshade::table::TableOps<Db>>(
                &mut self,
                rows: impl ::std::iter::IntoIterator<Item = T>,
            ) -> ::lightshade::Result<u64> {
                T::upsert_many(self, rows.into_iter().collect())
            }

            pub fn get<T: ::lightshade::table::TableOps<Db>>(&self, key: T::Key) -> ::lightshade::Result<::std::option::Option<T>> {
                T::get(self, key)
            }

            pub fn contains<T: ::lightshade::table::TableOps<Db>>(&self, key: &T::Key) -> ::lightshade::Result<bool> {
                T::contains(self, key)
            }

            pub fn update<T: ::lightshade::table::TableOps<Db>>(&mut self, row: T) -> ::lightshade::Result<bool> {
                T::update(self, row)
            }

            pub fn patch<T: ::lightshade::table::TableOps<Db>>(
                &mut self,
                key: T::Key,
                patch: T::Patch,
            ) -> ::lightshade::Result<bool> {
                T::patch(self, key, patch)
            }

            pub fn delete<T: ::lightshade::table::TableOps<Db>>(&mut self, key: T::Key) -> ::lightshade::Result<bool> {
                T::delete(self, key)
            }

            pub fn count<T: ::lightshade::table::TableOps<Db>>(&self) -> ::lightshade::Result<u64> {
                T::count(self)
            }

            pub fn scan<T: ::lightshade::table::TableOps<Db>>(&self) -> ::lightshade::Result<::std::vec::Vec<T>> {
                T::scan(self)
            }

            pub fn dump(
                &self,
                dest: impl ::std::convert::AsRef<::std::path::Path>,
            ) -> ::lightshade::Result<::lightshade::dataplane::DumpReport> {
                let tables = ::std::vec![
                    #(::lightshade::dataplane::DumpTable {
                        name: <#dataplane_tables as ::lightshade::Table>::NAME,
                        columns: <#dataplane_tables as ::lightshade::Table>::COLUMNS,
                        rows: self
                            .scan::<#dataplane_tables>()?
                            .into_iter()
                            .map(|row| <#dataplane_tables as ::lightshade::Table>::to_values(&row))
                            .collect(),
                    },)*
                ];
                ::lightshade::dataplane::write_dump_file(&tables, dest)
            }

            pub fn restore(
                db_path: impl ::std::convert::AsRef<::std::path::Path>,
                src: impl ::std::convert::AsRef<::std::path::Path>,
            ) -> ::lightshade::Result<::lightshade::dataplane::RestoreReport> {
                let dump = ::lightshade::dataplane::read_dump_file(src)?;
                ::lightshade::dataplane::check_tables(
                    &[#((<#dataplane_tables as ::lightshade::Table>::NAME, <#dataplane_tables as ::lightshade::Table>::COLUMNS),)*],
                    &dump,
                )?;
                let db_path = db_path.as_ref();
                if db_path.exists() {
                    return ::std::result::Result::Err(::lightshade::Error::InvalidArgument {
                        name: ::std::string::String::from("restore target"),
                        reason: ::std::format!("restore target `{}` already exists", db_path.display()),
                    });
                }
                let mut db = Db::open(db_path)?;
                let mut rows = 0u64;
                for table in &dump {
                    match table.name.as_str() {
                        #(#dataplane_names => {
                            let typed = table
                                .rows
                                .iter()
                                .map(|values| <#dataplane_tables as ::lightshade::Table>::from_values(values.clone()))
                                .collect::<::lightshade::Result<::std::vec::Vec<_>>>()?;
                            rows += typed.len() as u64;
                            db.insert_many::<#dataplane_tables>(typed)?;
                        })*
                        _ => ::std::unreachable!("dump passed validation but names an unknown table"),
                    }
                }
                db.close()?;
                ::std::result::Result::Ok(::lightshade::dataplane::RestoreReport {
                    tables: dump.len(),
                    rows,
                })
            }

            pub fn export_csv<T: ::lightshade::Table>(
                &self,
                dest: impl ::std::convert::AsRef<::std::path::Path>,
            ) -> ::lightshade::Result<::lightshade::dataplane::ExportReport> {
                ::lightshade::dataplane::export_csv::<T, Self>(self, dest)
            }

            pub fn import_csv<T: ::lightshade::Table>(
                &mut self,
                src: impl ::std::convert::AsRef<::std::path::Path>,
            ) -> ::lightshade::Result<::lightshade::dataplane::ImportReport> {
                ::lightshade::dataplane::import_csv::<T, Self>(self, src)
            }

            #(#operation_impls)*
        }

        #(#table_ops)*
        #[cfg(feature = "embedded")]
        #operation_trait
        #backend_impl
    }
}
