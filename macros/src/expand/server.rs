use super::ops::{operation_return, row_trait, OperationReturn};
use crate::lower::LoweredSchema;
use crate::model::{OperationKind, Schema};
use lightshadedb_codegen::ir::Ty;
use proc_macro2::TokenStream;
use quote::quote;
use syn::Type;

pub(crate) fn decode_param(param: &crate::model::Param, index: usize) -> TokenStream {
    let ident = &param.ident;
    let accessor = match param.ir {
        Ty::I8 => quote!(as_i8),
        Ty::I16 => quote!(as_i16),
        Ty::I32 => quote!(as_i32),
        Ty::I64 => quote!(as_i64),
        Ty::U8 => quote!(as_u8),
        Ty::U16 => quote!(as_u16),
        Ty::U32 => quote!(as_u32),
        Ty::U64 => quote!(as_u64),
        Ty::F32 => quote!(as_f32),
        Ty::F64 => quote!(as_f64),
        Ty::Bool => quote!(as_bool),
        Ty::Text => quote!(as_text),
        Ty::Bytes => quote!(as_bytes),
        Ty::Uuid => quote!(as_uuid),
    };
    quote! {
        let #ident = ::lightshadedb::wire::#accessor(
            ::lightshadedb::wire::param(params, #index)?
        )?;
    }
}

pub(crate) fn call_argument(param: &crate::model::Param) -> TokenStream {
    let ident = &param.ident;
    match param.ir {
        Ty::Text | Ty::Bytes => match &param.ty {
            Type::Reference(_) => quote!(&#ident),
            _ => quote!(#ident),
        },
        Ty::Uuid => match &param.ty {
            Type::Reference(_) => quote!(&#ident),
            _ => quote!(#ident),
        },
        _ => quote!(#ident),
    }
}

pub(crate) fn server_wrapper(
    schema: &Schema,
    lowered: &LoweredSchema,
    fingerprint: u64,
) -> TokenStream {
    let dispatch = server_dispatch(schema, lowered);
    let is_write = server_write_indices(schema, lowered);
    quote! {
        #[cfg(feature = "host")]
        fn __ls_is_write_operation(index: u32) -> bool {
            matches!(index, #(#is_write)|*)
        }

        #dispatch

        #[cfg(feature = "host")]
        pub struct Server {
            shutdown: ::std::sync::Arc<::std::sync::atomic::AtomicBool>,
            thread: ::std::option::Option<::std::thread::JoinHandle<::lightshadedb::Result<()>>>,
            sync_workers: ::std::vec::Vec<::std::thread::JoinHandle<()>>,
            endpoint: ::std::path::PathBuf,
            endpoint_value: ::lightshadedb::ClientEndpoint,
            local_addr: ::std::option::Option<::std::net::SocketAddr>,
            network_access: ::lightshadedb::NetworkAccess,
        }

        #[cfg(feature = "host")]
        pub struct ServedDb {
            pub name: String,
            pub db: Db,
        }

        #[cfg(feature = "host")]
        impl Server {
            pub fn start(db: Db, options: ::lightshadedb::ServeOptions) -> ::lightshadedb::Result<Self> {
                Self::start_all(
                    ::std::vec![ServedDb {
                        name: ::lightshadedb::DEFAULT_DATABASE.to_string(),
                        db,
                    }],
                    options,
                )
            }

            pub fn start_all(
                databases: Vec<ServedDb>,
                options: ::lightshadedb::ServeOptions,
            ) -> ::lightshadedb::Result<Self> {
                if databases.is_empty() {
                    return ::std::result::Result::Err(::lightshadedb::Error::InvalidArgument {
                        name: "databases".to_string(),
                        reason: "at least one database is required".to_string(),
                    });
                }
                let mut seen = ::std::collections::HashSet::new();
                for served in &databases {
                    if served.name.is_empty() {
                        return ::std::result::Result::Err(::lightshadedb::Error::InvalidArgument {
                            name: "database".to_string(),
                            reason: "database names must not be empty".to_string(),
                        });
                    }
                    if !seen.insert(served.name.clone()) {
                        return ::std::result::Result::Err(::lightshadedb::Error::InvalidArgument {
                            name: "database".to_string(),
                            reason: ::std::format!(
                                "duplicate database name `{}`",
                                served.name
                            ),
                        });
                    }
                    if served.db.path().as_os_str().is_empty() {
                        return ::std::result::Result::Err(::lightshadedb::Error::InvalidArgument {
                            name: "database".to_string(),
                            reason: "an in-memory database cannot be served".to_string(),
                        });
                    }
                }
                let base = ::lightshadedb::wire::load_credentials(&options.credentials)?;
                if base.is_empty() {
                    return ::std::result::Result::Err(::lightshadedb::Error::InvalidArgument {
                        name: "credentials".to_string(),
                        reason: "at least one credential is required".to_string(),
                    });
                }
                let base = ::std::sync::Arc::new(base);
                let first_path = databases
                    .first()
                    .map(|served| served.db.path().to_path_buf())
                    .unwrap_or_default();
                let mut entries = ::std::collections::HashMap::new();
                let shutdown = ::std::sync::Arc::new(::std::sync::atomic::AtomicBool::new(false));
                let mut sync_workers = ::std::vec::Vec::new();
                for served in databases {
                    let credentials = match options.credentials_overrides.get(&served.name) {
                        ::std::option::Option::Some(source) => {
                            let loaded =
                                ::lightshadedb::wire::load_credentials(source)?;
                            if loaded.is_empty() {
                                return ::std::result::Result::Err(
                                    ::lightshadedb::Error::InvalidArgument {
                                        name: "credentials".to_string(),
                                        reason: ::std::format!(
                                            "at least one credential is required for database `{}`",
                                            served.name
                                        ),
                                    },
                                );
                            }
                            ::std::sync::Arc::new(loaded)
                        }
                        ::std::option::Option::None => ::std::sync::Arc::clone(&base),
                    };
                    if let ::std::option::Option::Some(worker) =
                        served.db.__ls_start_group_commit(&shutdown)
                    {
                        sync_workers.push(worker);
                    }
                    entries.insert(
                        served.name,
                        __LsServed {
                            db: ::std::sync::RwLock::new(served.db),
                            credentials,
                            fingerprint: #fingerprint,
                        },
                    );
                }
                let bound = ::lightshadedb::wire::bind(&options.listen, &first_path)?;
                if let ::std::option::Option::Some(path) = &bound.endpoint_file {
                    ::lightshadedb::wire::write_endpoint_file(path, &bound.endpoint)?;
                }
                let endpoint = match &bound.endpoint {
                    ::lightshadedb::ClientEndpoint::Path(path) => path.clone(),
                    ::lightshadedb::ClientEndpoint::Tcp { .. } => bound
                        .endpoint_file
                        .clone()
                        .unwrap_or_else(|| ::lightshadedb::wire::endpoint_path(&first_path)),
                };
                let local_addr = bound.local_addr;
                let network_access = bound.network_access;
                let listener = bound.listener;
                let flag = ::std::sync::Arc::clone(&shutdown);
                let databases = ::std::sync::Arc::new(entries);
                let access = network_access;
                let allow = ::std::sync::Arc::new(bound.allowlist);
                let thread = ::std::thread::spawn(move || {
                    while !flag.load(::std::sync::atomic::Ordering::SeqCst) {
                        match listener.accept() {
                            ::std::result::Result::Ok(mut stream) => {
                                if !::lightshadedb::wire::allows_peer(access, &allow, &stream) {
                                    continue;
                                }
                                let _ = stream.set_timeout(::std::option::Option::Some(
                                    ::std::time::Duration::from_secs(30),
                                ));
                                let databases = ::std::sync::Arc::clone(&databases);
                                let flag = ::std::sync::Arc::clone(&flag);
                                ::std::thread::spawn(move || {
                                    __ls_serve_connection(stream, databases, flag);
                                });
                            }
                            ::std::result::Result::Err(error)
                                if error.kind() == ::std::io::ErrorKind::WouldBlock =>
                            {
                                ::std::thread::sleep(::std::time::Duration::from_millis(20));
                            }
                            ::std::result::Result::Err(error) => {
                                return ::std::result::Result::Err(::lightshadedb::Error::io(
                                    "accepting a connection",
                                    error,
                                ));
                            }
                        }
                    }
                    ::std::result::Result::Ok(())
                });
                Ok(Self {
                    shutdown,
                    thread: ::std::option::Option::Some(thread),
                    sync_workers,
                    endpoint,
                    endpoint_value: bound.endpoint,
                    local_addr,
                    network_access,
                })
            }

            pub fn endpoint(&self) -> &::std::path::Path {
                &self.endpoint
            }

            pub fn endpoint_value(&self) -> &::lightshadedb::ClientEndpoint {
                &self.endpoint_value
            }

            pub fn local_addr(&self) -> ::std::option::Option<::std::net::SocketAddr> {
                self.local_addr
            }

            pub fn network_access(&self) -> ::lightshadedb::NetworkAccess {
                self.network_access
            }

            pub fn shutdown(&self) {
                self.shutdown
                    .store(true, ::std::sync::atomic::Ordering::SeqCst);
            }

            pub fn join(mut self) -> ::lightshadedb::Result<()> {
                self.shutdown();
                let result = match self.thread.take() {
                    ::std::option::Option::Some(thread) => thread
                        .join()
                        .map_err(|_| ::lightshadedb::Error::Failed {
                            reason: "server thread panicked".to_string(),
                        })?,
                    ::std::option::Option::None => ::std::result::Result::Ok(()),
                };
                for worker in self.sync_workers.drain(..) {
                    let _ = worker.join();
                }
                let _ = ::std::fs::remove_file(&self.endpoint);
                result
            }
        }

        #[cfg(feature = "host")]
        impl ::std::ops::Drop for Server {
            fn drop(&mut self) {
                self.shutdown();
            }
        }

        #[cfg(feature = "host")]
        struct __LsServed {
            db: ::std::sync::RwLock<Db>,
            credentials: ::std::sync::Arc<::lightshadedb::Credentials>,
            fingerprint: u64,
        }

        #[cfg(feature = "host")]
        fn __ls_serve_connection(
            mut stream: ::lightshadedb::wire::Stream,
            databases: ::std::sync::Arc<
                ::std::collections::HashMap<String, __LsServed>,
            >,
            shutdown: ::std::sync::Arc<::std::sync::atomic::AtomicBool>,
        ) {
            let hello = match ::lightshadedb::wire::read_hello(&mut stream) {
                ::std::result::Result::Ok(hello) => hello,
                ::std::result::Result::Err(error) => {
                    let _ = ::lightshadedb::wire::write_handshake_error(&mut stream, &error);
                    return;
                }
            };
            let Some(entry) = databases.get(&hello.database) else {
                let _ = ::lightshadedb::wire::write_handshake_error(
                    &mut stream,
                    &::lightshadedb::Error::Auth(::lightshadedb::AuthError::Failed),
                );
                return;
            };
            let role = match ::lightshadedb::wire::authenticate(
                &hello,
                entry.fingerprint,
                &entry.credentials,
            ) {
                ::std::result::Result::Ok(role) => role,
                ::std::result::Result::Err(error) => {
                    let _ = ::lightshadedb::wire::write_handshake_error(&mut stream, &error);
                    return;
                }
            };
            if ::lightshadedb::wire::write_handshake_ok(&mut stream).is_err() {
                return;
            }
            loop {
                if shutdown.load(::std::sync::atomic::Ordering::SeqCst) {
                    return;
                }
                let (kind, index, params) = match ::lightshadedb::wire::read_request(&mut stream) {
                    ::std::result::Result::Ok(request) => request,
                    ::std::result::Result::Err(::lightshadedb::Error::Io { .. }) => return,
                    ::std::result::Result::Err(error) => {
                        let _ = ::lightshadedb::wire::write_response_error(&mut stream, &error);
                        return;
                    }
                };
                let result = if kind == ::lightshadedb::wire::RequestKind::Operation
                    && !role.can_write()
                    && __ls_is_write_operation(index)
                {
                    ::std::result::Result::Err(::lightshadedb::Error::Auth(
                        ::lightshadedb::AuthError::PermissionDenied,
                    ))
                } else if kind == ::lightshadedb::wire::RequestKind::Operation
                    && !__ls_is_write_operation(index)
                {
                    let db = match entry.db.read() {
                        ::std::result::Result::Ok(db) => db,
                        ::std::result::Result::Err(poisoned) => poisoned.into_inner(),
                    };
                    __ls_server_dispatch_read(&db, index, &params)
                } else {
                    let (result, waited) = {
                        let mut db = match entry.db.write() {
                            ::std::result::Result::Ok(db) => db,
                            ::std::result::Result::Err(poisoned) => poisoned.into_inner(),
                        };
                        let __ls_before = db.__ls_current_lsn();
                        let result = match kind {
                            ::lightshadedb::wire::RequestKind::Operation => {
                                __ls_server_dispatch_write(&mut db, index, &params)
                            }
                            ::lightshadedb::wire::RequestKind::Begin => {
                                db.begin().map(|()| ::std::vec::Vec::new())
                            }
                            ::lightshadedb::wire::RequestKind::Commit => {
                                db.commit().map(|()| ::std::vec::Vec::new())
                            }
                            ::lightshadedb::wire::RequestKind::Rollback => {
                                db.rollback().map(|()| ::std::vec::Vec::new())
                            }
                        };
                        let __ls_after = db.__ls_current_lsn();
                        (
                            result,
                            (__ls_after > __ls_before).then_some(__ls_after),
                        )
                    };
                    if let ::std::option::Option::Some(__ls_lsn) = waited {
                        let __ls_sync = {
                            let db = match entry.db.read() {
                                ::std::result::Result::Ok(db) => db,
                                ::std::result::Result::Err(poisoned) => poisoned.into_inner(),
                            };
                            db.__ls_wal_sync_handle()
                        };
                        if let ::std::option::Option::Some(__ls_sync) = __ls_sync {
                            if let ::std::result::Result::Err(__ls_error) =
                                __lightshadedb::__ls_wait_on_sync(&__ls_sync, __ls_lsn)
                            {
                                let _ = ::lightshadedb::wire::write_response_error(
                                    &mut stream,
                                    &::lightshadedb::Error::io(
                                        "waiting for group commit",
                                        __ls_error,
                                    ),
                                );
                                return;
                            }
                        }
                    }
                    result
                };
                match result {
                    ::std::result::Result::Ok(values) => {
                        if ::lightshadedb::wire::write_response_ok(&mut stream, &values).is_err() {
                            return;
                        }
                    }
                    ::std::result::Result::Err(error) => {
                        if ::lightshadedb::wire::write_response_error(&mut stream, &error).is_err() {
                            return;
                        }
                    }
                }
            }
        }

        #[cfg(feature = "host")]
        impl Db {
            pub fn serve(self, options: ::lightshadedb::ServeOptions) -> ::lightshadedb::Result<()> {
                let server = Server::start(self, options)?;
                server.join()
            }
        }
    }
}

pub(crate) fn server_write_indices(schema: &Schema, lowered: &LoweredSchema) -> Vec<u32> {
    let mut indices = Vec::new();
    if let Some(operations) = &schema.operations {
        for (index, operation) in operations.methods.iter().enumerate() {
            if matches!(
                operation.kind,
                OperationKind::Update(_) | OperationKind::Delete(_)
            ) {
                indices.push(index as u32);
            }
        }
    }
    let base = schema
        .operations
        .as_ref()
        .map(|ops| ops.methods.len())
        .unwrap_or(0);
    for table_index in 0..lowered.tables.len() {
        for slot in [0usize, 1, 2, 3, 6, 7] {
            indices.push((base + table_index * 10 + slot) as u32);
        }
    }
    indices
}

pub(crate) fn server_dispatch(schema: &Schema, lowered: &LoweredSchema) -> TokenStream {
    let base = schema
        .operations
        .as_ref()
        .map(|ops| ops.methods.len())
        .unwrap_or(0);
    let (user_read_arms, user_write_arms): (Vec<_>, Vec<_>) = schema
        .operations
        .as_ref()
        .map(|operations| {
            operations
                .methods
                .iter()
                .enumerate()
                .map(|(index, operation)| {
                    let index = index as u32;
                    let name = &operation.method.sig.ident;
                    let decode = operation.params.iter().enumerate().map(|(position, param)| {
                        decode_param(param, position)
                    });
                    let args = operation.params.iter().map(call_argument);
                    let encode = match operation_return(operation, schema).expect("validated") {
                        OperationReturn::Rows(row, projection) => {
                            let trait_ = row_trait(projection);
                            quote! {
                                let rows = db.#name(#(#args),*)?;
                                let rows = rows
                                    .iter()
                                    .map(|row| <#row as #trait_>::to_values(row))
                                    .collect::<::std::vec::Vec<_>>();
                                ::std::result::Result::Ok(::lightshadedb::wire::encode_rows(&rows))
                            }
                        }
                        OperationReturn::Count => quote! {
                            let value = db.#name(#(#args),*)?;
                            ::std::result::Result::Ok(::std::vec![::lightshadedb::table::Value::I64(value)])
                        },
                        OperationReturn::Affected => quote! {
                            let value = db.#name(#(#args),*)?;
                            ::std::result::Result::Ok(::std::vec![::lightshadedb::table::Value::U64(value)])
                        },
                    };
                    let read = matches!(
                        operation.kind,
                        OperationKind::Query(_) | OperationKind::Count(_)
                    );
                    (
                        read,
                        quote! {
                            #index => {
                                #(#decode)*
                                #encode
                            }
                        },
                    )
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
        .into_iter()
        .partition(|(read, _)| *read);
    let user_read_arms = user_read_arms
        .into_iter()
        .map(|(_, arm)| arm)
        .collect::<Vec<_>>();
    let user_write_arms = user_write_arms
        .into_iter()
        .map(|(_, arm)| arm)
        .collect::<Vec<_>>();
    let (crud_read_arms, crud_write_arms): (Vec<_>, Vec<_>) = lowered
        .tables
        .iter()
        .enumerate()
        .flat_map(|(table_index, table)| {
            let db_name = &table.db_name;
            let width = table.columns.len();
            [0usize, 1, 2, 3, 4, 5, 6, 7, 8, 9]
                .into_iter()
                .map(move |slot| {
                    let index = (base + table_index * 10 + slot) as u32;
                    let body = match slot {
                        0 => quote! {
                            ::lightshadedb::Backend::__insert(db, #db_name, params.to_vec())?;
                            ::std::result::Result::Ok(::std::vec::Vec::new())
                        },
                        1 => quote! {
                            let rows = ::lightshadedb::wire::decode_rows(params.to_vec(), #width)?;
                            ::lightshadedb::Backend::__insert_many(db, #db_name, rows)?;
                            ::std::result::Result::Ok(::std::vec::Vec::new())
                        },
                        2 => quote! {
                            let inserted = ::lightshadedb::Backend::__upsert(db, #db_name, params.to_vec())?;
                            ::std::result::Result::Ok(::std::vec![::lightshadedb::table::Value::Bool(inserted)])
                        },
                        3 => quote! {
                            let rows = ::lightshadedb::wire::decode_rows(params.to_vec(), #width)?;
                            let inserted = ::lightshadedb::Backend::__upsert_many(db, #db_name, rows)?;
                            ::std::result::Result::Ok(::std::vec![::lightshadedb::table::Value::U64(inserted)])
                        },
                        4 => quote! {
                            let row = ::lightshadedb::Backend::__get(db, #db_name, params[0].clone())?;
                            let mut values = ::std::vec![::lightshadedb::table::Value::U8(row.is_some() as u8)];
                            if let ::std::option::Option::Some(row) = row {
                                values.extend(row);
                            }
                            ::std::result::Result::Ok(values)
                        },
                        5 => quote! {
                            let contains = ::lightshadedb::Backend::__contains(db, #db_name, params[0].clone())?;
                            ::std::result::Result::Ok(::std::vec![::lightshadedb::table::Value::Bool(contains)])
                        },
                        6 => quote! {
                            let updated = ::lightshadedb::Backend::__update(db, #db_name, params.to_vec())?;
                            ::std::result::Result::Ok(::std::vec![::lightshadedb::table::Value::Bool(updated)])
                        },
                        7 => quote! {
                            let deleted = ::lightshadedb::Backend::__delete(db, #db_name, params[0].clone())?;
                            ::std::result::Result::Ok(::std::vec![::lightshadedb::table::Value::Bool(deleted)])
                        },
                        8 => quote! {
                            let count = ::lightshadedb::Backend::__count(db, #db_name)?;
                            ::std::result::Result::Ok(::std::vec![::lightshadedb::table::Value::U64(count)])
                        },
                        9 => quote! {
                            let rows = ::lightshadedb::Backend::__scan(db, #db_name)?;
                            ::std::result::Result::Ok(::lightshadedb::wire::encode_rows(&rows))
                        },
                        _ => unreachable!(),
                    };
                    let read = matches!(slot, 4 | 5 | 8 | 9);
                    (
                        read,
                        quote!(#index => { #body }),
                    )
                })
        })
        .collect::<Vec<_>>()
        .into_iter()
        .partition(|(read, _)| *read);
    let crud_read_arms = crud_read_arms
        .into_iter()
        .map(|(_, arm)| arm)
        .collect::<Vec<_>>();
    let crud_write_arms = crud_write_arms
        .into_iter()
        .map(|(_, arm)| arm)
        .collect::<Vec<_>>();
    quote! {
        #[cfg(feature = "host")]
        fn __ls_assert_db_send_sync() {
            fn __ls_assert<T: Send + Sync>() {}
            __ls_assert::<Db>();
        }
        #[cfg(feature = "host")]
        #[allow(unused_variables)]
        fn __ls_server_dispatch_read(
            db: &Db,
            index: u32,
            params: &[::lightshadedb::table::Value],
        ) -> ::lightshadedb::Result<::std::vec::Vec<::lightshadedb::table::Value>> {
            match index {
                #(#user_read_arms)*
                #(#crud_read_arms,)*
                other => ::std::result::Result::Err(::lightshadedb::Error::InvalidArgument {
                    name: "operation".to_string(),
                    reason: ::std::format!("unknown operation index {other}"),
                }),
            }
        }
        #[cfg(feature = "host")]
        #[allow(unused_variables)]
        fn __ls_server_dispatch_write(
            db: &mut Db,
            index: u32,
            params: &[::lightshadedb::table::Value],
        ) -> ::lightshadedb::Result<::std::vec::Vec<::lightshadedb::table::Value>> {
            match index {
                #(#user_write_arms)*
                #(#crud_write_arms,)*
                other => ::std::result::Result::Err(::lightshadedb::Error::InvalidArgument {
                    name: "operation".to_string(),
                    reason: ::std::format!("unknown operation index {other}"),
                }),
            }
        }
    }
}
