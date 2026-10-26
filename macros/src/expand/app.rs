use crate::lower::LoweredSchema;
use crate::model::{OperationKind, Schema};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use super::ops::operation_trait_impls;
pub(crate) fn app_wrapper(schema: &Schema, lowered: &LoweredSchema) -> TokenStream {
    let trait_bound = if let Some(operations) = &schema.operations {
        let ident = &operations.ident;
        quote!(#ident + ::lightshadedb::Backend)
    } else {
        quote!(::lightshadedb::Backend)
    };
    let operation_impls = if let Some(operations) = &schema.operations {
        let impls = operations
            .methods
            .iter()
            .map(|operation| {
                let name = &operation.method.sig.ident;
                let receiver = match operation.kind {
                    OperationKind::Query(_) | OperationKind::Count(_) => quote!(&self),
                    _ => quote!(&mut self),
                };
                let params = operation.params.iter().map(|param| {
                    let ident = &param.ident;
                    let ty = &param.ty;
                    quote!(#ident: #ty)
                });
                let args = operation
                    .params
                    .iter()
                    .map(|param| &param.ident)
                    .collect::<Vec<_>>();
                let output = &operation.method.sig.output;
                quote! {
                    pub fn #name(#receiver, #(#params),*) #output {
                        match self {
                            #[cfg(feature = "embedded")]
                            App::Embedded(db) => Db::#name(db, #(#args),*),
                            #[cfg(feature = "client")]
                            App::Client(client) => Client::#name(client, #(#args),*),
                        }
                    }
                }
            })
            .collect::<Vec<_>>();
        quote!(#(#impls)*)
    } else {
        TokenStream::new()
    };
    let op_trait = operation_trait_impls(schema, &format_ident!("App"));
    let backend = backend_impl_for_app();
    let _ = lowered;
    quote! {
        pub enum App {
            #[cfg(feature = "embedded")]
            Embedded(Db),
            #[cfg(feature = "client")]
            Client(Client),
        }

        pub enum AppConfig {
            #[cfg(feature = "embedded")]
            Embedded {
                path: ::std::path::PathBuf,
                options: ::lightshadedb::OpenOptions,
            },
            #[cfg(feature = "client")]
            Client {
                endpoint: ::lightshadedb::ClientEndpoint,
                options: ::lightshadedb::ClientOptions,
            },
        }

        impl AppConfig {
            #[cfg(feature = "embedded")]
            pub fn embedded(path: impl ::std::convert::Into<::std::path::PathBuf>) -> Self {
                Self::Embedded {
                    path: path.into(),
                    options: ::lightshadedb::OpenOptions::default(),
                }
            }

            #[cfg(feature = "client")]
            pub fn client(
                endpoint: ::lightshadedb::ClientEndpoint,
                options: ::lightshadedb::ClientOptions,
            ) -> Self {
                Self::Client { endpoint, options }
            }
        }

        impl App {
            pub fn open(config: AppConfig) -> ::lightshadedb::Result<App> {
                match config {
                    #[cfg(feature = "embedded")]
                    AppConfig::Embedded { path, options } => {
                        ::std::result::Result::Ok(App::Embedded(Db::open_with(path, options)?))
                    }
                    #[cfg(feature = "client")]
                    AppConfig::Client { endpoint, options } => {
                        ::std::result::Result::Ok(App::Client(Client::connect_endpoint(
                            endpoint, options,
                        )?))
                    }
                }
            }

            pub fn from_env() -> ::lightshadedb::Result<App> {
                let mode = ::std::env::var("LIGHTSHADE_MODE").unwrap_or_else(|_| {
                    #[cfg(feature = "embedded")]
                    {
                        "embedded".to_string()
                    }
                    #[cfg(not(feature = "embedded"))]
                    {
                        "client".to_string()
                    }
                });
                match mode.as_str() {
                    #[cfg(feature = "embedded")]
                    "embedded" => {
                        let path = ::std::env::var("LIGHTSHADE_PATH").map_err(|_| {
                            ::lightshadedb::Error::InvalidArgument {
                                name: "LIGHTSHADE_PATH".to_string(),
                                reason: "is not set".to_string(),
                            }
                        })?;
                        ::std::result::Result::Ok(App::Embedded(Db::open(path)?))
                    }
                    #[cfg(feature = "client")]
                    "client" => {
                        let endpoint = ::std::env::var("LIGHTSHADE_ENDPOINT").map_err(|_| {
                            ::lightshadedb::Error::InvalidArgument {
                                name: "LIGHTSHADE_ENDPOINT".to_string(),
                                reason: "is not set".to_string(),
                            }
                        })?;
                        let (endpoint, options) = if endpoint.starts_with("lightshadedb://") {
                            let spec = ::lightshadedb::ConnectionSpec::parse(&endpoint)?;
                            ::lightshadedb::ClientOptions::from_spec_or_env(&spec)?
                        } else {
                            let endpoint = ::lightshadedb::ClientEndpoint::parse(&endpoint)?;
                            (endpoint, ::lightshadedb::ClientOptions::from_env()?)
                        };
                        ::std::result::Result::Ok(App::Client(Client::connect_endpoint(
                            endpoint, options,
                        )?))
                    }
                    other => ::std::result::Result::Err(::lightshadedb::Error::InvalidArgument {
                        name: "LIGHTSHADE_MODE".to_string(),
                        reason: ::std::format!("unsupported mode `{other}`"),
                    }),
                }
            }

            pub fn db_path(&self) -> ::std::option::Option<&::std::path::Path> {
                match self {
                    #[cfg(feature = "embedded")]
                    App::Embedded(db) => ::std::option::Option::Some(db.path()),
                    #[cfg(feature = "client")]
                    App::Client(_) => ::std::option::Option::None,
                }
            }

            pub fn client_endpoint(&self) -> ::std::option::Option<&::lightshadedb::ClientEndpoint> {
                match self {
                    #[cfg(feature = "embedded")]
                    App::Embedded(_) => ::std::option::Option::None,
                    #[cfg(feature = "client")]
                    App::Client(client) => ::std::option::Option::Some(client.endpoint()),
                }
            }

            pub fn remote_addr(&self) -> ::std::option::Option<::std::net::SocketAddr> {
                match self {
                    #[cfg(feature = "embedded")]
                    App::Embedded(_) => ::std::option::Option::None,
                    #[cfg(feature = "client")]
                    App::Client(client) => client.remote_addr(),
                }
            }

            pub fn in_transaction(&self) -> bool {
                match self {
                    #[cfg(feature = "embedded")]
                    App::Embedded(db) => db.in_transaction(),
                    #[cfg(feature = "client")]
                    App::Client(client) => client.in_transaction(),
                }
            }

            pub fn begin(&mut self) -> ::lightshadedb::Result<()> {
                match self {
                    #[cfg(feature = "embedded")]
                    App::Embedded(db) => db.begin(),
                    #[cfg(feature = "client")]
                    App::Client(client) => client.begin(),
                }
            }

            pub fn commit(&mut self) -> ::lightshadedb::Result<()> {
                match self {
                    #[cfg(feature = "embedded")]
                    App::Embedded(db) => db.commit(),
                    #[cfg(feature = "client")]
                    App::Client(client) => client.commit(),
                }
            }

            pub fn rollback(&mut self) -> ::lightshadedb::Result<()> {
                match self {
                    #[cfg(feature = "embedded")]
                    App::Embedded(db) => db.rollback(),
                    #[cfg(feature = "client")]
                    App::Client(client) => client.rollback(),
                }
            }

            pub fn transaction<T>(
                &mut self,
                f: impl ::std::ops::FnOnce(&mut App) -> ::lightshadedb::Result<T>,
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

            pub fn get<T: ::lightshadedb::Table>(
                &self,
                key: T::Key,
            ) -> ::lightshadedb::Result<::std::option::Option<T>> {
                ::lightshadedb::Backend::get::<T>(self, key)
            }

            pub fn contains<T: ::lightshadedb::Table>(
                &self,
                key: &T::Key,
            ) -> ::lightshadedb::Result<bool> {
                ::lightshadedb::Backend::contains::<T>(self, key)
            }

            pub fn update<T: ::lightshadedb::Table>(&mut self, row: T) -> ::lightshadedb::Result<bool> {
                ::lightshadedb::Backend::update(self, row)
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

            pub fn flush(&mut self) -> ::lightshadedb::Result<()> {
                match self {
                    #[cfg(feature = "embedded")]
                    App::Embedded(db) => db.flush(),
                    #[cfg(feature = "client")]
                    App::Client(_) => ::std::result::Result::Err(::lightshadedb::Error::Unsupported {
                        feature: "embedded",
                    }),
                }
            }

            pub fn checkpoint(&mut self) -> ::lightshadedb::Result<()> {
                match self {
                    #[cfg(feature = "embedded")]
                    App::Embedded(db) => db.checkpoint(),
                    #[cfg(feature = "client")]
                    App::Client(_) => ::std::result::Result::Err(::lightshadedb::Error::Unsupported {
                        feature: "embedded",
                    }),
                }
            }

            pub fn close(self) -> ::lightshadedb::Result<()> {
                match self {
                    #[cfg(feature = "embedded")]
                    App::Embedded(db) => db.close(),
                    #[cfg(feature = "client")]
                    App::Client(client) => client.close(),
                }
            }

            #operation_impls
        }

        #op_trait
        #backend

        pub trait Backend: #trait_bound {}

        #[cfg(feature = "embedded")]
        impl Backend for Db {}

        #[cfg(feature = "client")]
        impl Backend for Client {}

        impl Backend for App {}
    }
}

pub(crate) fn backend_impl_for_app() -> TokenStream {
    quote! {
        impl ::lightshadedb::Backend for App {
            fn __insert(&mut self, table: &'static str, values: ::std::vec::Vec<::lightshadedb::table::Value>) -> ::lightshadedb::Result<()> {
                match self {
                    #[cfg(feature = "embedded")]
                    App::Embedded(db) => ::lightshadedb::Backend::__insert(db, table, values),
                    #[cfg(feature = "client")]
                    App::Client(client) => ::lightshadedb::Backend::__insert(client, table, values),
                }
            }
            fn __insert_many(&mut self, table: &'static str, rows: ::std::vec::Vec<::std::vec::Vec<::lightshadedb::table::Value>>) -> ::lightshadedb::Result<()> {
                match self {
                    #[cfg(feature = "embedded")]
                    App::Embedded(db) => ::lightshadedb::Backend::__insert_many(db, table, rows),
                    #[cfg(feature = "client")]
                    App::Client(client) => ::lightshadedb::Backend::__insert_many(client, table, rows),
                }
            }
            fn __upsert(&mut self, table: &'static str, values: ::std::vec::Vec<::lightshadedb::table::Value>) -> ::lightshadedb::Result<bool> {
                match self {
                    #[cfg(feature = "embedded")]
                    App::Embedded(db) => ::lightshadedb::Backend::__upsert(db, table, values),
                    #[cfg(feature = "client")]
                    App::Client(client) => ::lightshadedb::Backend::__upsert(client, table, values),
                }
            }
            fn __upsert_many(&mut self, table: &'static str, rows: ::std::vec::Vec<::std::vec::Vec<::lightshadedb::table::Value>>) -> ::lightshadedb::Result<u64> {
                match self {
                    #[cfg(feature = "embedded")]
                    App::Embedded(db) => ::lightshadedb::Backend::__upsert_many(db, table, rows),
                    #[cfg(feature = "client")]
                    App::Client(client) => ::lightshadedb::Backend::__upsert_many(client, table, rows),
                }
            }
            fn __get(&self, table: &'static str, key: ::lightshadedb::table::Value) -> ::lightshadedb::Result<::std::option::Option<::std::vec::Vec<::lightshadedb::table::Value>>> {
                match self {
                    #[cfg(feature = "embedded")]
                    App::Embedded(db) => ::lightshadedb::Backend::__get(db, table, key),
                    #[cfg(feature = "client")]
                    App::Client(client) => ::lightshadedb::Backend::__get(client, table, key),
                }
            }
            fn __contains(&self, table: &'static str, key: ::lightshadedb::table::Value) -> ::lightshadedb::Result<bool> {
                match self {
                    #[cfg(feature = "embedded")]
                    App::Embedded(db) => ::lightshadedb::Backend::__contains(db, table, key),
                    #[cfg(feature = "client")]
                    App::Client(client) => ::lightshadedb::Backend::__contains(client, table, key),
                }
            }
            fn __update(&mut self, table: &'static str, values: ::std::vec::Vec<::lightshadedb::table::Value>) -> ::lightshadedb::Result<bool> {
                match self {
                    #[cfg(feature = "embedded")]
                    App::Embedded(db) => ::lightshadedb::Backend::__update(db, table, values),
                    #[cfg(feature = "client")]
                    App::Client(client) => ::lightshadedb::Backend::__update(client, table, values),
                }
            }
            fn __delete(&mut self, table: &'static str, key: ::lightshadedb::table::Value) -> ::lightshadedb::Result<bool> {
                match self {
                    #[cfg(feature = "embedded")]
                    App::Embedded(db) => ::lightshadedb::Backend::__delete(db, table, key),
                    #[cfg(feature = "client")]
                    App::Client(client) => ::lightshadedb::Backend::__delete(client, table, key),
                }
            }
            fn __count(&self, table: &'static str) -> ::lightshadedb::Result<u64> {
                match self {
                    #[cfg(feature = "embedded")]
                    App::Embedded(db) => ::lightshadedb::Backend::__count(db, table),
                    #[cfg(feature = "client")]
                    App::Client(client) => ::lightshadedb::Backend::__count(client, table),
                }
            }
            fn __scan(&self, table: &'static str) -> ::lightshadedb::Result<::std::vec::Vec<::std::vec::Vec<::lightshadedb::table::Value>>> {
                match self {
                    #[cfg(feature = "embedded")]
                    App::Embedded(db) => ::lightshadedb::Backend::__scan(db, table),
                    #[cfg(feature = "client")]
                    App::Client(client) => ::lightshadedb::Backend::__scan(client, table),
                }
            }
            fn begin(&mut self) -> ::lightshadedb::Result<()> {
                App::begin(self)
            }
            fn commit(&mut self) -> ::lightshadedb::Result<()> {
                App::commit(self)
            }
            fn rollback(&mut self) -> ::lightshadedb::Result<()> {
                App::rollback(self)
            }
            fn in_transaction(&self) -> bool {
                App::in_transaction(self)
            }
            fn transaction<T, F>(&mut self, f: F) -> ::lightshadedb::Result<T>
            where F: ::std::ops::FnOnce(&mut Self) -> ::lightshadedb::Result<T>, Self: ::std::marker::Sized {
                App::transaction(self, f)
            }
            fn close(self) -> ::lightshadedb::Result<()> {
                App::close(self)
            }
        }
    }
}
