use super::ops::{operation_return, operation_trait_impls, row_trait, OperationReturn};
use crate::lower::LoweredSchema;
use crate::model::{Operation, OperationKind, Schema};
use lightshadedb_codegen::ir::Ty;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::Type;

#[derive(Clone, Copy)]
pub(crate) enum NetCall {
    Sync,
    Pool,
    Pooled,
}

pub(crate) fn net_call(call: NetCall, index: TokenStream, args: TokenStream) -> TokenStream {
    match call {
        NetCall::Sync => quote!(self.__ls_request(#index, #args)?),
        NetCall::Pool => quote!(self.pool.request(#index, #args).await?),
        NetCall::Pooled => quote!(self.pooled.request(
            ::lightshadedb::wire::RequestKind::Operation,
            #index,
            #args
        ).await?),
    }
}

pub(crate) fn net_request(call: NetCall, index: TokenStream, args: TokenStream) -> TokenStream {
    let call = net_call(call, index, args);
    quote!(let values = #call;)
}

pub(crate) fn operation_decode(operation: &Operation, schema: &Schema) -> TokenStream {
    match operation_return(operation, schema).expect("validated return") {
        OperationReturn::Rows(row, projection) => {
            let trait_ = row_trait(projection);
            let width = if projection {
                quote!(<#row as ::lightshadedb::Projection>::FIELDS.len())
            } else {
                quote!(<#row as ::lightshadedb::Table>::COLUMNS.len())
            };
            quote! {
                let width = #width;
                let rows = ::lightshadedb::wire::decode_rows(values, width)?;
                let mut output = ::std::vec::Vec::with_capacity(rows.len());
                for row in rows {
                    output.push(<#row as #trait_>::from_values(row)?);
                }
                ::std::result::Result::Ok(output)
            }
        }
        OperationReturn::Count => quote! {
            let value = ::lightshadedb::wire::as_i64(
                ::lightshadedb::wire::param(&values, 0)?
            )?;
            ::std::result::Result::Ok(value)
        },
        OperationReturn::Affected => quote! {
            let value = ::lightshadedb::wire::as_u64(
                ::lightshadedb::wire::param(&values, 0)?
            )?;
            ::std::result::Result::Ok(value)
        },
    }
}

pub(crate) fn user_op_method(
    operation: &Operation,
    schema: &Schema,
    receiver: TokenStream,
    is_async: bool,
    request: TokenStream,
) -> TokenStream {
    let name = &operation.method.sig.ident;
    let params = operation.params.iter().map(|param| {
        let ident = &param.ident;
        let ty = &param.ty;
        quote!(#ident: #ty)
    });
    let values = operation.params.iter().map(param_value);
    let output = &operation.method.sig.output;
    let decode = operation_decode(operation, schema);
    let asyncness = is_async.then(|| quote!(async));
    quote! {
        pub #asyncness fn #name(#receiver, #(#params),*) #output {
            let params = ::std::vec![#(#values,)*];
            #request
            #decode
        }
    }
}

pub(crate) fn user_op_methods(
    schema: &Schema,
    receiver: impl Fn(&Operation) -> TokenStream,
    is_async: bool,
    call: NetCall,
) -> Vec<TokenStream> {
    let empty_methods = Vec::new();
    schema
        .operations
        .as_ref()
        .map(|operations| &operations.methods)
        .unwrap_or(&empty_methods)
        .iter()
        .enumerate()
        .map(|(index, operation)| {
            user_op_method(
                operation,
                schema,
                receiver(operation),
                is_async,
                net_request(call, quote!(#index as u32), quote!(&params)),
            )
        })
        .collect()
}

pub(crate) fn param_value(param: &crate::model::Param) -> TokenStream {
    let ident = &param.ident;
    match param.ir {
        Ty::I8 => quote!(::lightshadedb::table::Value::I8(#ident)),
        Ty::I16 => quote!(::lightshadedb::table::Value::I16(#ident)),
        Ty::I32 => quote!(::lightshadedb::table::Value::I32(#ident)),
        Ty::I64 => quote!(::lightshadedb::table::Value::I64(#ident)),
        Ty::U8 => quote!(::lightshadedb::table::Value::U8(#ident)),
        Ty::U16 => quote!(::lightshadedb::table::Value::U16(#ident)),
        Ty::U32 => quote!(::lightshadedb::table::Value::U32(#ident)),
        Ty::U64 => quote!(::lightshadedb::table::Value::U64(#ident)),
        Ty::F32 => quote!(::lightshadedb::table::Value::F32(#ident)),
        Ty::F64 => quote!(::lightshadedb::table::Value::F64(#ident)),
        Ty::Bool => quote!(::lightshadedb::table::Value::Bool(#ident)),
        Ty::Text => quote!(::lightshadedb::table::Value::Text(#ident.to_string())),
        Ty::Bytes => quote!(::lightshadedb::table::Value::Bytes(#ident.to_vec())),
        Ty::Uuid => match &param.ty {
            Type::Reference(_) => quote!(::lightshadedb::table::Value::Uuid(*#ident)),
            _ => quote!(::lightshadedb::table::Value::Uuid(#ident)),
        },
    }
}

pub(crate) fn crud_arms(
    base: usize,
    lowered: &LoweredSchema,
    slot: u32,
    call: NetCall,
) -> Vec<TokenStream> {
    lowered
        .tables
        .iter()
        .enumerate()
        .map(|(table_index, table)| {
            let index = (base + table_index * 10 + slot as usize) as u32;
            let width = table.columns.len();
            let db_name = &table.db_name;
            let request = |args: TokenStream| net_call(call, quote!(#index), args);
            let body = match slot {
                0 => {
                    let call = request(quote!(&values));
                    quote! {
                        #call;
                        ::std::result::Result::Ok(())
                    }
                }
                1 => {
                    let call = request(quote!(&request));
                    quote! {
                        let mut request = ::std::vec![::lightshadedb::table::Value::U32(rows.len() as u32)];
                        for row in &rows { request.extend(row.iter().cloned()); }
                        #call;
                        ::std::result::Result::Ok(())
                    }
                }
                2 => {
                    let call = request(quote!(&values));
                    quote! {
                        let values = #call;
                        ::std::result::Result::Ok(::lightshadedb::wire::as_bool(::lightshadedb::wire::param(&values, 0)?)?)
                    }
                }
                3 => {
                    let call = request(quote!(&request));
                    quote! {
                        let mut request = ::std::vec![::lightshadedb::table::Value::U32(rows.len() as u32)];
                        for row in &rows { request.extend(row.iter().cloned()); }
                        let values = #call;
                        ::std::result::Result::Ok(::lightshadedb::wire::as_u64(::lightshadedb::wire::param(&values, 0)?)?)
                    }
                }
                4 => {
                    let call = request(quote!(&[key]));
                    quote! {
                        let values = #call;
                        if ::lightshadedb::wire::as_u8(::lightshadedb::wire::param(&values, 0)?)? == 0 {
                            return ::std::result::Result::Ok(::std::option::Option::None);
                        }
                        let row = values.into_iter().skip(1).collect::<::std::vec::Vec<_>>();
                        ::std::result::Result::Ok(::std::option::Option::Some(row))
                    }
                }
                5 => {
                    let call = request(quote!(&[key]));
                    quote! {
                        let values = #call;
                        ::std::result::Result::Ok(::lightshadedb::wire::as_bool(::lightshadedb::wire::param(&values, 0)?)?)
                    }
                }
                6 => {
                    let call = request(quote!(&values));
                    quote! {
                        let values = #call;
                        ::std::result::Result::Ok(::lightshadedb::wire::as_bool(::lightshadedb::wire::param(&values, 0)?)?)
                    }
                }
                7 => {
                    let call = request(quote!(&[key]));
                    quote! {
                        let values = #call;
                        ::std::result::Result::Ok(::lightshadedb::wire::as_bool(::lightshadedb::wire::param(&values, 0)?)?)
                    }
                }
                8 => {
                    let call = request(quote!(&[]));
                    quote! {
                        let values = #call;
                        ::std::result::Result::Ok(::lightshadedb::wire::as_u64(::lightshadedb::wire::param(&values, 0)?)?)
                    }
                }
                9 => {
                    let call = request(quote!(&[]));
                    quote! {
                        let values = #call;
                        ::lightshadedb::wire::decode_rows(values, #width)
                    }
                }
                _ => unreachable!(),
            };
            quote!(#db_name => { #body })
        })
        .collect()
}

pub(crate) struct CrudArms {
    insert: Vec<TokenStream>,
    insert_many: Vec<TokenStream>,
    upsert: Vec<TokenStream>,
    upsert_many: Vec<TokenStream>,
    get: Vec<TokenStream>,
    contains: Vec<TokenStream>,
    update: Vec<TokenStream>,
    delete: Vec<TokenStream>,
    count: Vec<TokenStream>,
    scan: Vec<TokenStream>,
}

pub(crate) fn crud_arms_for(base: usize, lowered: &LoweredSchema, call: NetCall) -> CrudArms {
    CrudArms {
        insert: crud_arms(base, lowered, 0, call),
        insert_many: crud_arms(base, lowered, 1, call),
        upsert: crud_arms(base, lowered, 2, call),
        upsert_many: crud_arms(base, lowered, 3, call),
        get: crud_arms(base, lowered, 4, call),
        contains: crud_arms(base, lowered, 5, call),
        update: crud_arms(base, lowered, 6, call),
        delete: crud_arms(base, lowered, 7, call),
        count: crud_arms(base, lowered, 8, call),
        scan: crud_arms(base, lowered, 9, call),
    }
}

pub(crate) fn unknown_table_arm() -> TokenStream {
    quote! {
        other => ::std::result::Result::Err(::lightshadedb::Error::Schema(
            ::lightshadedb::SchemaError::UnknownTable { name: other.to_string() },
        ))
    }
}

pub(crate) fn async_crud_methods(arms: &CrudArms, unknown: &TokenStream) -> Vec<TokenStream> {
    let CrudArms {
        insert,
        insert_many,
        upsert,
        upsert_many,
        get,
        contains,
        update,
        delete,
        count,
        scan,
    } = arms;
    vec![
        quote! {
            pub async fn insert<T: ::lightshadedb::Table>(&self, row: T) -> ::lightshadedb::Result<()> {
                let values = <T as ::lightshadedb::Table>::to_values(&row);
                match <T as ::lightshadedb::Table>::NAME {
                    #(#insert,)*
                    #unknown,
                }
            }
        },
        quote! {
            pub async fn insert_many<T: ::lightshadedb::Table>(
                &self,
                rows: impl ::std::iter::IntoIterator<Item = T>,
            ) -> ::lightshadedb::Result<()> {
                let rows = rows
                    .into_iter()
                    .map(|row| <T as ::lightshadedb::Table>::to_values(&row))
                    .collect::<::std::vec::Vec<_>>();
                match <T as ::lightshadedb::Table>::NAME {
                    #(#insert_many,)*
                    #unknown,
                }
            }
        },
        quote! {
            pub async fn upsert<T: ::lightshadedb::Table>(&self, row: T) -> ::lightshadedb::Result<bool> {
                let values = <T as ::lightshadedb::Table>::to_values(&row);
                match <T as ::lightshadedb::Table>::NAME {
                    #(#upsert,)*
                    #unknown,
                }
            }
        },
        quote! {
            pub async fn upsert_many<T: ::lightshadedb::Table>(
                &self,
                rows: impl ::std::iter::IntoIterator<Item = T>,
            ) -> ::lightshadedb::Result<u64> {
                let rows = rows
                    .into_iter()
                    .map(|row| <T as ::lightshadedb::Table>::to_values(&row))
                    .collect::<::std::vec::Vec<_>>();
                match <T as ::lightshadedb::Table>::NAME {
                    #(#upsert_many,)*
                    #unknown,
                }
            }
        },
        quote! {
            pub async fn get<T: ::lightshadedb::Table>(
                &self,
                key: T::Key,
            ) -> ::lightshadedb::Result<::std::option::Option<T>> {
                let key = <T as ::lightshadedb::Table>::key_to_value(&key);
                let found = match <T as ::lightshadedb::Table>::NAME {
                    #(#get,)*
                    #unknown,
                }?;
                match found {
                    ::std::option::Option::Some(values) => ::std::result::Result::Ok(
                        ::std::option::Option::Some(<T as ::lightshadedb::Table>::from_values(values)?),
                    ),
                    ::std::option::Option::None => ::std::result::Result::Ok(::std::option::Option::None),
                }
            }
        },
        quote! {
            pub async fn contains<T: ::lightshadedb::Table>(
                &self,
                key: &T::Key,
            ) -> ::lightshadedb::Result<bool> {
                let key = <T as ::lightshadedb::Table>::key_to_value(key);
                match <T as ::lightshadedb::Table>::NAME {
                    #(#contains,)*
                    #unknown,
                }
            }
        },
        quote! {
            pub async fn update<T: ::lightshadedb::Table>(&self, row: T) -> ::lightshadedb::Result<bool> {
                let values = <T as ::lightshadedb::Table>::to_values(&row);
                match <T as ::lightshadedb::Table>::NAME {
                    #(#update,)*
                    #unknown,
                }
            }
        },
        quote! {
            pub async fn patch<T: ::lightshadedb::Table>(
                &self,
                key: T::Key,
                patch: T::Patch,
            ) -> ::lightshadedb::Result<bool> {
                let ::std::option::Option::Some(mut row) = self.get::<T>(key).await? else {
                    return ::std::result::Result::Ok(false);
                };
                <T as ::lightshadedb::Table>::apply_patch(&mut row, &patch);
                self.update(row).await
            }
        },
        quote! {
            pub async fn delete<T: ::lightshadedb::Table>(
                &self,
                key: T::Key,
            ) -> ::lightshadedb::Result<bool> {
                let key = <T as ::lightshadedb::Table>::key_to_value(&key);
                match <T as ::lightshadedb::Table>::NAME {
                    #(#delete,)*
                    #unknown,
                }
            }
        },
        quote! {
            pub async fn count<T: ::lightshadedb::Table>(&self) -> ::lightshadedb::Result<u64> {
                match <T as ::lightshadedb::Table>::NAME {
                    #(#count,)*
                    #unknown,
                }
            }
        },
        quote! {
            pub async fn scan<T: ::lightshadedb::Table>(
                &self,
            ) -> ::lightshadedb::Result<::std::vec::Vec<T>> {
                let rows: ::std::vec::Vec<::std::vec::Vec<::lightshadedb::table::Value>> =
                    match <T as ::lightshadedb::Table>::NAME {
                        #(#scan,)*
                        #unknown,
                    }?;
                let mut output = ::std::vec::Vec::with_capacity(rows.len());
                for values in rows {
                    output.push(<T as ::lightshadedb::Table>::from_values(values)?);
                }
                ::std::result::Result::Ok(output)
            }
        },
    ]
}

pub(crate) fn client_wrapper(
    schema: &Schema,
    lowered: &LoweredSchema,
    fingerprint: u64,
) -> TokenStream {
    let base = schema
        .operations
        .as_ref()
        .map_or(0, |operations| operations.methods.len());
    let user_methods = user_op_methods(
        schema,
        |operation| match operation.kind {
            OperationKind::Query(_) | OperationKind::Count(_) => quote!(&self),
            _ => quote!(&mut self),
        },
        false,
        NetCall::Sync,
    );

    let crud_call = NetCall::Sync;
    let insert = crud_arms(base, lowered, 0, crud_call);
    let insert_many = crud_arms(base, lowered, 1, crud_call);
    let upsert = crud_arms(base, lowered, 2, crud_call);
    let upsert_many = crud_arms(base, lowered, 3, crud_call);
    let get = crud_arms(base, lowered, 4, crud_call);
    let contains = crud_arms(base, lowered, 5, crud_call);
    let update = crud_arms(base, lowered, 6, crud_call);
    let delete = crud_arms(base, lowered, 7, crud_call);
    let count = crud_arms(base, lowered, 8, crud_call);
    let scan = crud_arms(base, lowered, 9, crud_call);
    let op_trait = operation_trait_impls(schema, &format_ident!("Client"));
    let unknown = quote! {
        other => ::std::result::Result::Err(::lightshadedb::Error::Schema(
            ::lightshadedb::SchemaError::UnknownTable { name: other.to_string() },
        ))
    };

    quote! {
        #[cfg(feature = "client")]
        struct __LsClientInner {
            stream: ::lightshadedb::wire::Stream,
            in_transaction: bool,
        }

        #[cfg(feature = "client")]
        pub struct Client {
            inner: ::std::cell::RefCell<__LsClientInner>,
            endpoint: ::lightshadedb::ClientEndpoint,
            remote_addr: ::std::option::Option<::std::net::SocketAddr>,
        }

        #[cfg(feature = "client")]
        impl Client {
            pub fn connect(
                endpoint: impl ::std::convert::AsRef<::std::path::Path>,
                options: ::lightshadedb::ClientOptions,
            ) -> ::lightshadedb::Result<Self> {
                Self::connect_endpoint(
                    ::lightshadedb::ClientEndpoint::path(endpoint.as_ref()),
                    options,
                )
            }

            pub fn connect_endpoint(
                endpoint: ::lightshadedb::ClientEndpoint,
                options: ::lightshadedb::ClientOptions,
            ) -> ::lightshadedb::Result<Self> {
                let (mut stream, endpoint) = ::lightshadedb::wire::connect(
                    &endpoint,
                    options.connect_timeout,
                    options.io_timeout,
                )?;
                ::lightshadedb::wire::client_handshake(
                    &mut stream,
                    #fingerprint,
                    &options.credentials,
                    endpoint.database(),
                )?;
                let remote_addr = stream.peer_addr();
                Ok(Self {
                    inner: ::std::cell::RefCell::new(__LsClientInner {
                        stream,
                        in_transaction: false,
                    }),
                    endpoint,
                    remote_addr,
                })
            }

            pub fn connect_tcp(
                addr: impl ::std::net::ToSocketAddrs,
                options: ::lightshadedb::ClientOptions,
            ) -> ::lightshadedb::Result<Self> {
                Self::connect_tcp_db(addr, ::lightshadedb::DEFAULT_DATABASE, options)
            }

            pub fn connect_tcp_db(
                addr: impl ::std::net::ToSocketAddrs,
                database: &str,
                options: ::lightshadedb::ClientOptions,
            ) -> ::lightshadedb::Result<Self> {
                use ::std::net::TcpStream;
                let addresses = addr
                    .to_socket_addrs()
                    .map_err(|source| ::lightshadedb::Error::io("resolving the TCP address", source))?;
                let mut last = ::std::option::Option::None;
                for address in addresses {
                    match TcpStream::connect_timeout(&address, options.connect_timeout) {
                        ::std::result::Result::Ok(tcp) => {
                            tcp.set_nodelay(true).ok();
                            let mut stream = ::lightshadedb::wire::Stream::Tcp(tcp);
                            stream.set_timeout(::std::option::Option::Some(options.io_timeout))?;
                            if let ::std::result::Result::Err(error) =
                                ::lightshadedb::wire::client_handshake(
                                    &mut stream,
                                    #fingerprint,
                                    &options.credentials,
                                    database,
                                )
                            {
                                return ::std::result::Result::Err(error);
                            }
                            return Ok(Self {
                                inner: ::std::cell::RefCell::new(__LsClientInner {
                                    stream,
                                    in_transaction: false,
                                }),
                                endpoint: ::lightshadedb::ClientEndpoint::tcp_db(
                                    address.ip().to_string(),
                                    address.port(),
                                    database,
                                ),
                                remote_addr: ::std::option::Option::Some(address),
                            });
                        }
                        ::std::result::Result::Err(error) => last = ::std::option::Option::Some(error),
                    }
                }
                ::std::result::Result::Err(::lightshadedb::Error::io(
                    "connecting to the Lightshade DB server",
                    last.unwrap_or_else(|| ::std::io::Error::new(
                        ::std::io::ErrorKind::AddrNotAvailable,
                        "address resolved to no sockets",
                    )),
                ))
            }

            pub fn endpoint(&self) -> &::lightshadedb::ClientEndpoint {
                &self.endpoint
            }

            pub fn path(&self) -> ::std::option::Option<&::std::path::Path> {
                self.endpoint.as_path()
            }

            pub fn remote_addr(&self) -> ::std::option::Option<::std::net::SocketAddr> {
                self.remote_addr
            }

            pub fn in_transaction(&self) -> bool {
                self.inner.borrow().in_transaction
            }

            pub fn close(self) -> ::lightshadedb::Result<()> {
                let _ = self.inner.into_inner().stream;
                Ok(())
            }

            fn __ls_request(
                &self,
                index: u32,
                params: &[::lightshadedb::table::Value],
            ) -> ::lightshadedb::Result<::std::vec::Vec<::lightshadedb::table::Value>> {
                let mut inner = self.inner.borrow_mut();
                ::lightshadedb::wire::request(
                    &mut inner.stream,
                    ::lightshadedb::wire::RequestKind::Operation,
                    index,
                    params,
                )
            }

            pub fn begin(&mut self) -> ::lightshadedb::Result<()> {
                let mut inner = self.inner.borrow_mut();
                ::lightshadedb::wire::request(
                    &mut inner.stream,
                    ::lightshadedb::wire::RequestKind::Begin,
                    0,
                    &[],
                )?;
                inner.in_transaction = true;
                Ok(())
            }

            pub fn commit(&mut self) -> ::lightshadedb::Result<()> {
                let mut inner = self.inner.borrow_mut();
                ::lightshadedb::wire::request(
                    &mut inner.stream,
                    ::lightshadedb::wire::RequestKind::Commit,
                    0,
                    &[],
                )?;
                inner.in_transaction = false;
                Ok(())
            }

            pub fn rollback(&mut self) -> ::lightshadedb::Result<()> {
                let mut inner = self.inner.borrow_mut();
                ::lightshadedb::wire::request(
                    &mut inner.stream,
                    ::lightshadedb::wire::RequestKind::Rollback,
                    0,
                    &[],
                )?;
                inner.in_transaction = false;
                Ok(())
            }

            pub fn transaction<T>(
                &mut self,
                f: impl ::std::ops::FnOnce(&mut Client) -> ::lightshadedb::Result<T>,
            ) -> ::lightshadedb::Result<T> {
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

            pub fn insert<T: ::lightshadedb::Table>(&mut self, row: T) -> ::lightshadedb::Result<()> {
                ::lightshadedb::Backend::insert(self, row)
            }

            pub fn insert_many<T: ::lightshadedb::Table>(
                &mut self,
                rows: impl ::std::iter::IntoIterator<Item = T>,
            ) -> ::lightshadedb::Result<()> {
                ::lightshadedb::Backend::insert_many(self, rows)
            }

            pub fn upsert<T: ::lightshadedb::Table>(&mut self, row: T) -> ::lightshadedb::Result<bool> {
                ::lightshadedb::Backend::upsert(self, row)
            }

            pub fn upsert_many<T: ::lightshadedb::Table>(
                &mut self,
                rows: impl ::std::iter::IntoIterator<Item = T>,
            ) -> ::lightshadedb::Result<u64> {
                ::lightshadedb::Backend::upsert_many(self, rows)
            }

            pub fn get<T: ::lightshadedb::Table>(&self, key: T::Key) -> ::lightshadedb::Result<::std::option::Option<T>> {
                ::lightshadedb::Backend::get::<T>(self, key)
            }

            pub fn contains<T: ::lightshadedb::Table>(&self, key: &T::Key) -> ::lightshadedb::Result<bool> {
                ::lightshadedb::Backend::contains::<T>(self, key)
            }

            pub fn update<T: ::lightshadedb::Table>(&mut self, row: T) -> ::lightshadedb::Result<bool> {
                ::lightshadedb::Backend::update::<T>(self, row)
            }

            pub fn patch<T: ::lightshadedb::Table>(
                &mut self,
                key: T::Key,
                patch: T::Patch,
            ) -> ::lightshadedb::Result<bool> {
                ::lightshadedb::Backend::patch::<T>(self, key, patch)
            }

            pub fn delete<T: ::lightshadedb::Table>(&mut self, key: T::Key) -> ::lightshadedb::Result<bool> {
                ::lightshadedb::Backend::delete::<T>(self, key)
            }

            pub fn count<T: ::lightshadedb::Table>(&self) -> ::lightshadedb::Result<u64> {
                ::lightshadedb::Backend::count::<T>(self)
            }

            pub fn scan<T: ::lightshadedb::Table>(&self) -> ::lightshadedb::Result<::std::vec::Vec<T>> {
                ::lightshadedb::Backend::scan::<T>(self)
            }

            #(#user_methods)*
        }

        #[cfg(feature = "client")]
        impl ::lightshadedb::Backend for Client {
            fn __insert(&mut self, table: &'static str, values: ::std::vec::Vec<::lightshadedb::table::Value>) -> ::lightshadedb::Result<()> {
                match table {
                    #(#insert,)*
                    #unknown,
                }
            }
            fn __insert_many(&mut self, table: &'static str, rows: ::std::vec::Vec<::std::vec::Vec<::lightshadedb::table::Value>>) -> ::lightshadedb::Result<()> {
                match table {
                    #(#insert_many,)*
                    #unknown,
                }
            }
            fn __upsert(&mut self, table: &'static str, values: ::std::vec::Vec<::lightshadedb::table::Value>) -> ::lightshadedb::Result<bool> {
                match table {
                    #(#upsert,)*
                    #unknown,
                }
            }
            fn __upsert_many(&mut self, table: &'static str, rows: ::std::vec::Vec<::std::vec::Vec<::lightshadedb::table::Value>>) -> ::lightshadedb::Result<u64> {
                match table {
                    #(#upsert_many,)*
                    #unknown,
                }
            }
            fn __get(&self, table: &'static str, key: ::lightshadedb::table::Value) -> ::lightshadedb::Result<::std::option::Option<::std::vec::Vec<::lightshadedb::table::Value>>> {
                match table {
                    #(#get,)*
                    #unknown,
                }
            }
            fn __contains(&self, table: &'static str, key: ::lightshadedb::table::Value) -> ::lightshadedb::Result<bool> {
                match table {
                    #(#contains,)*
                    #unknown,
                }
            }
            fn __update(&mut self, table: &'static str, values: ::std::vec::Vec<::lightshadedb::table::Value>) -> ::lightshadedb::Result<bool> {
                match table {
                    #(#update,)*
                    #unknown,
                }
            }
            fn __delete(&mut self, table: &'static str, key: ::lightshadedb::table::Value) -> ::lightshadedb::Result<bool> {
                match table {
                    #(#delete,)*
                    #unknown,
                }
            }
            fn __count(&self, table: &'static str) -> ::lightshadedb::Result<u64> {
                match table {
                    #(#count,)*
                    #unknown,
                }
            }
            fn __scan(&self, table: &'static str) -> ::lightshadedb::Result<::std::vec::Vec<::std::vec::Vec<::lightshadedb::table::Value>>> {
                match table {
                    #(#scan,)*
                    #unknown,
                }
            }
            fn begin(&mut self) -> ::lightshadedb::Result<()> { Client::begin(self) }
            fn commit(&mut self) -> ::lightshadedb::Result<()> { Client::commit(self) }
            fn rollback(&mut self) -> ::lightshadedb::Result<()> { Client::rollback(self) }
            fn in_transaction(&self) -> bool { Client::in_transaction(self) }
            fn transaction<T, F>(&mut self, f: F) -> ::lightshadedb::Result<T>
            where F: ::std::ops::FnOnce(&mut Self) -> ::lightshadedb::Result<T>, Self: ::std::marker::Sized {
                Client::transaction(self, f)
            }
            fn close(self) -> ::lightshadedb::Result<()> { Client::close(self) }
        }

        #[cfg(feature = "client")]
        #op_trait
    }
}

pub(crate) fn async_client_wrapper(
    schema: &Schema,
    lowered: &LoweredSchema,
    fingerprint: u64,
) -> TokenStream {
    let base = schema
        .operations
        .as_ref()
        .map_or(0, |operations| operations.methods.len());
    let async_ops = user_op_methods(schema, |_| quote!(&self), true, NetCall::Pool);
    let txn_ops = user_op_methods(schema, |_| quote!(&self), true, NetCall::Pooled);
    let async_arms = crud_arms_for(base, lowered, NetCall::Pool);
    let txn_arms = crud_arms_for(base, lowered, NetCall::Pooled);
    let unknown = unknown_table_arm();
    let async_crud = async_crud_methods(&async_arms, &unknown);
    let txn_crud = async_crud_methods(&txn_arms, &unknown);
    let pool_options = quote!(::lightshadedb::pool::PoolOptions);

    quote! {
        #[cfg(all(feature = "client", feature = "async"))]
        #[derive(Clone, Debug)]
        pub struct AsyncClient {
            pool: ::lightshadedb::pool::Pool,
        }

        #[cfg(all(feature = "client", feature = "async"))]
        impl AsyncClient {
            pub async fn connect(
                endpoint: impl ::std::convert::AsRef<::std::path::Path>,
                options: #pool_options,
            ) -> ::lightshadedb::Result<Self> {
                Self::connect_endpoint(::lightshadedb::ClientEndpoint::path(endpoint.as_ref()), options).await
            }

            pub async fn connect_endpoint(
                endpoint: ::lightshadedb::ClientEndpoint,
                options: #pool_options,
            ) -> ::lightshadedb::Result<Self> {
                let pool = ::lightshadedb::pool::Pool::new(endpoint, #fingerprint, options).await?;
                ::std::result::Result::Ok(Self { pool })
            }

            pub async fn connect_tcp(
                addr: impl ::std::net::ToSocketAddrs,
                options: #pool_options,
            ) -> ::lightshadedb::Result<Self> {
                Self::connect_tcp_db(addr, ::lightshadedb::DEFAULT_DATABASE, options).await
            }

            pub async fn connect_tcp_db(
                addr: impl ::std::net::ToSocketAddrs,
                database: &str,
                options: #pool_options,
            ) -> ::lightshadedb::Result<Self> {
                let addresses = addr
                    .to_socket_addrs()
                    .map_err(|source| ::lightshadedb::Error::io("resolving the TCP address", source))?;
                let mut last = ::std::option::Option::None;
                for address in addresses {
                    let endpoint = ::lightshadedb::ClientEndpoint::tcp_db(
                        address.ip().to_string(),
                        address.port(),
                        database,
                    );
                    match ::lightshadedb::pool::Pool::new(endpoint, #fingerprint, options.clone()).await {
                        ::std::result::Result::Ok(pool) => {
                            return ::std::result::Result::Ok(Self { pool });
                        }
                        ::std::result::Result::Err(error) => {
                            last = ::std::option::Option::Some(error);
                        }
                    }
                }
                ::std::result::Result::Err(last.unwrap_or_else(|| {
                    ::lightshadedb::Error::io(
                        "connecting to the Lightshade DB server",
                        ::std::io::Error::new(
                            ::std::io::ErrorKind::AddrNotAvailable,
                            "address resolved to no sockets",
                        ),
                    )
                }))
            }

            pub fn pool(&self) -> &::lightshadedb::pool::Pool {
                &self.pool
            }

            pub fn endpoint(&self) -> &::lightshadedb::ClientEndpoint {
                self.pool.endpoint()
            }

            pub async fn begin(&self) -> ::lightshadedb::Result<Transaction> {
                let pooled = self.pool.get().await?;
                pooled
                    .request(::lightshadedb::wire::RequestKind::Begin, 0, &[])
                    .await?;
                ::std::result::Result::Ok(Transaction { pooled, open: true })
            }

            #(#async_crud)*
            #(#async_ops)*
        }

        #[cfg(all(feature = "client", feature = "async"))]
        pub struct Transaction {
            pooled: ::lightshadedb::pool::Pooled,
            open: bool,
        }

        #[cfg(all(feature = "client", feature = "async"))]
        impl Transaction {
            pub async fn commit(mut self) -> ::lightshadedb::Result<()> {
                let result = self
                    .pooled
                    .request(::lightshadedb::wire::RequestKind::Commit, 0, &[])
                    .await;
                if result.is_ok() {
                    self.open = false;
                }
                result.map(|_| ())
            }

            pub async fn rollback(mut self) -> ::lightshadedb::Result<()> {
                let result = self
                    .pooled
                    .request(::lightshadedb::wire::RequestKind::Rollback, 0, &[])
                    .await;
                if result.is_ok() {
                    self.open = false;
                }
                result.map(|_| ())
            }

            #(#txn_crud)*
            #(#txn_ops)*
        }

        #[cfg(all(feature = "client", feature = "async"))]
        impl ::std::ops::Drop for Transaction {
            fn drop(&mut self) {
                if self.open {
                    self.pooled.defunct();
                }
            }
        }
    }
}
