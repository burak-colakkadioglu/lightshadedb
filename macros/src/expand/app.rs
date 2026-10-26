use crate::lower::LoweredSchema;
use crate::model::{OperationKind, Schema};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use super::ops::operation_trait_impls;
pub(crate) fn app_wrapper(schema: &Schema, lowered: &LoweredSchema) -> TokenStream {
    let trait_bound = if let Some(operations) = &schema.operations {
        let ident = &operations.ident;
        quote!(#ident + ::lightshade::Backend)
    } else {
        quote!(::lightshade::Backend)
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
                options: ::lightshade::OpenOptions,
            },
            #[cfg(feature = "client")]
            Client {
                endpoint: ::lightshade::ClientEndpoint,
                options: ::lightshade::ClientOptions,
            },
        }

        impl AppConfig {
            #[cfg(feature = "embedded")]
            pub fn embedded(path: impl ::std::convert::Into<::std::path::PathBuf>) -> Self {
                Self::Embedded {
                    path: path.into(),
                    options: ::lightshade::OpenOptions::default(),
                }
            }

            #[cfg(feature = "client")]
            pub fn client(
                endpoint: ::lightshade::ClientEndpoint,
                options: ::lightshade::ClientOptions,
            ) -> Self {
                Self::Client { endpoint, options }
            }
        }

        impl App {
            pub fn open(config: AppConfig) -> ::lightshade::Result<App> {
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

            pub fn from_env() -> ::lightshade::Result<App> {
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
                            ::lightshade::Error::InvalidArgument {
                                name: "LIGHTSHADE_PATH".to_string(),
                                reason: "is not set".to_string(),
                            }
                        })?;
                        ::std::result::Result::Ok(App::Embedded(Db::open(path)?))
                    }
                    #[cfg(feature = "client")]
                    "client" => {
                        let endpoint = ::std::env::var("LIGHTSHADE_ENDPOINT").map_err(|_| {
                            ::lightshade::Error::InvalidArgument {
                                name: "LIGHTSHADE_ENDPOINT".to_string(),
                                reason: "is not set".to_string(),
                            }
                        })?;
                        let endpoint = ::lightshade::ClientEndpoint::parse(&endpoint)?;
                        let options = ::lightshade::ClientOptions::from_env()?;
                        ::std::result::Result::Ok(App::Client(Client::connect_endpoint(
                            endpoint, options,
                        )?))
                    }
                    other => ::std::result::Result::Err(::lightshade::Error::InvalidArgument {
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

            pub fn client_endpoint(&self) -> ::std::option::Option<&::lightshade::ClientEndpoint> {
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

            pub fn begin(&mut self) -> ::lightshade::Result<()> {
                match self {
                    #[cfg(feature = "embedded")]
                    App::Embedded(db) => db.begin(),
                    #[cfg(feature = "client")]
                    App::Client(client) => client.begin(),
                }
            }

            pub fn commit(&mut self) -> ::lightshade::Result<()> {
                match self {
                    #[cfg(feature = "embedded")]
                    App::Embedded(db) => db.commit(),
                    #[cfg(feature = "client")]
                    App::Client(client) => client.commit(),
                }
            }

            pub fn rollback(&mut self) -> ::lightshade::Result<()> {
                match self {
                    #[cfg(feature = "embedded")]
                    App::Embedded(db) => db.rollback(),
                    #[cfg(feature = "client")]
                    App::Client(client) => client.rollback(),
                }
            }

            pub fn transaction<T>(
                &mut self,
                f: impl ::std::ops::FnOnce(&mut App) -> ::lightshade::Result<T>,
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

            pub fn insert<T: ::lightshade::Table>(&mut self, row: T) -> ::lightshade::Result<()> {
                ::lightshade::Backend::insert(self, row)
            }

            pub fn insert_many<T: ::lightshade::Table>(
                &mut self,
                rows: impl ::std::iter::IntoIterator<Item = T>,
            ) -> ::lightshade::Result<()> {
                ::lightshade::Backend::insert_many(self, rows)
            }

            pub fn upsert<T: ::lightshade::Table>(&mut self, row: T) -> ::lightshade::Result<bool> {
                ::lightshade::Backend::upsert(self, row)
            }

            pub fn upsert_many<T: ::lightshade::Table>(
                &mut self,
                rows: impl ::std::iter::IntoIterator<Item = T>,
            ) -> ::lightshade::Result<u64> {
                ::lightshade::Backend::upsert_many(self, rows)
            }

            pub fn get<T: ::lightshade::Table>(
                &self,
                key: T::Key,
            ) -> ::lightshade::Result<::std::option::Option<T>> {
                ::lightshade::Backend::get::<T>(self, key)
            }

            pub fn contains<T: ::lightshade::Table>(
                &self,
                key: &T::Key,
            ) -> ::lightshade::Result<bool> {
                ::lightshade::Backend::contains::<T>(self, key)
            }

            pub fn update<T: ::lightshade::Table>(&mut self, row: T) -> ::lightshade::Result<bool> {
                ::lightshade::Backend::update(self, row)
            }

            pub fn patch<T: ::lightshade::Table>(
                &mut self,
                key: T::Key,
                patch: T::Patch,
            ) -> ::lightshade::Result<bool> {
                ::lightshade::Backend::patch::<T>(self, key, patch)
            }

            pub fn delete<T: ::lightshade::Table>(&mut self, key: T::Key) -> ::lightshade::Result<bool> {
                ::lightshade::Backend::delete::<T>(self, key)
            }

            pub fn count<T: ::lightshade::Table>(&self) -> ::lightshade::Result<u64> {
                ::lightshade::Backend::count::<T>(self)
            }

            pub fn scan<T: ::lightshade::Table>(&self) -> ::lightshade::Result<::std::vec::Vec<T>> {
                ::lightshade::Backend::scan::<T>(self)
            }

            pub fn flush(&mut self) -> ::lightshade::Result<()> {
                match self {
                    #[cfg(feature = "embedded")]
                    App::Embedded(db) => db.flush(),
                    #[cfg(feature = "client")]
                    App::Client(_) => ::std::result::Result::Err(::lightshade::Error::Unsupported {
                        feature: "embedded",
                    }),
                }
            }

            pub fn checkpoint(&mut self) -> ::lightshade::Result<()> {
                match self {
                    #[cfg(feature = "embedded")]
                    App::Embedded(db) => db.checkpoint(),
                    #[cfg(feature = "client")]
                    App::Client(_) => ::std::result::Result::Err(::lightshade::Error::Unsupported {
                        feature: "embedded",
                    }),
                }
            }

            pub fn close(self) -> ::lightshade::Result<()> {
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
        impl ::lightshade::Backend for App {
            fn __insert(&mut self, table: &'static str, values: ::std::vec::Vec<::lightshade::table::Value>) -> ::lightshade::Result<()> {
                match self {
                    #[cfg(feature = "embedded")]
                    App::Embedded(db) => ::lightshade::Backend::__insert(db, table, values),
                    #[cfg(feature = "client")]
                    App::Client(client) => ::lightshade::Backend::__insert(client, table, values),
                }
            }
            fn __insert_many(&mut self, table: &'static str, rows: ::std::vec::Vec<::std::vec::Vec<::lightshade::table::Value>>) -> ::lightshade::Result<()> {
                match self {
                    #[cfg(feature = "embedded")]
                    App::Embedded(db) => ::lightshade::Backend::__insert_many(db, table, rows),
                    #[cfg(feature = "client")]
                    App::Client(client) => ::lightshade::Backend::__insert_many(client, table, rows),
                }
            }
            fn __upsert(&mut self, table: &'static str, values: ::std::vec::Vec<::lightshade::table::Value>) -> ::lightshade::Result<bool> {
                match self {
                    #[cfg(feature = "embedded")]
                    App::Embedded(db) => ::lightshade::Backend::__upsert(db, table, values),
                    #[cfg(feature = "client")]
                    App::Client(client) => ::lightshade::Backend::__upsert(client, table, values),
                }
            }
            fn __upsert_many(&mut self, table: &'static str, rows: ::std::vec::Vec<::std::vec::Vec<::lightshade::table::Value>>) -> ::lightshade::Result<u64> {
                match self {
                    #[cfg(feature = "embedded")]
                    App::Embedded(db) => ::lightshade::Backend::__upsert_many(db, table, rows),
                    #[cfg(feature = "client")]
                    App::Client(client) => ::lightshade::Backend::__upsert_many(client, table, rows),
                }
            }
            fn __get(&self, table: &'static str, key: ::lightshade::table::Value) -> ::lightshade::Result<::std::option::Option<::std::vec::Vec<::lightshade::table::Value>>> {
                match self {
                    #[cfg(feature = "embedded")]
                    App::Embedded(db) => ::lightshade::Backend::__get(db, table, key),
                    #[cfg(feature = "client")]
                    App::Client(client) => ::lightshade::Backend::__get(client, table, key),
                }
            }
            fn __contains(&self, table: &'static str, key: ::lightshade::table::Value) -> ::lightshade::Result<bool> {
                match self {
                    #[cfg(feature = "embedded")]
                    App::Embedded(db) => ::lightshade::Backend::__contains(db, table, key),
                    #[cfg(feature = "client")]
                    App::Client(client) => ::lightshade::Backend::__contains(client, table, key),
                }
            }
            fn __update(&mut self, table: &'static str, values: ::std::vec::Vec<::lightshade::table::Value>) -> ::lightshade::Result<bool> {
                match self {
                    #[cfg(feature = "embedded")]
                    App::Embedded(db) => ::lightshade::Backend::__update(db, table, values),
                    #[cfg(feature = "client")]
                    App::Client(client) => ::lightshade::Backend::__update(client, table, values),
                }
            }
            fn __delete(&mut self, table: &'static str, key: ::lightshade::table::Value) -> ::lightshade::Result<bool> {
                match self {
                    #[cfg(feature = "embedded")]
                    App::Embedded(db) => ::lightshade::Backend::__delete(db, table, key),
                    #[cfg(feature = "client")]
                    App::Client(client) => ::lightshade::Backend::__delete(client, table, key),
                }
            }
            fn __count(&self, table: &'static str) -> ::lightshade::Result<u64> {
                match self {
                    #[cfg(feature = "embedded")]
                    App::Embedded(db) => ::lightshade::Backend::__count(db, table),
                    #[cfg(feature = "client")]
                    App::Client(client) => ::lightshade::Backend::__count(client, table),
                }
            }
            fn __scan(&self, table: &'static str) -> ::lightshade::Result<::std::vec::Vec<::std::vec::Vec<::lightshade::table::Value>>> {
                match self {
                    #[cfg(feature = "embedded")]
                    App::Embedded(db) => ::lightshade::Backend::__scan(db, table),
                    #[cfg(feature = "client")]
                    App::Client(client) => ::lightshade::Backend::__scan(client, table),
                }
            }
            fn begin(&mut self) -> ::lightshade::Result<()> {
                App::begin(self)
            }
            fn commit(&mut self) -> ::lightshade::Result<()> {
                App::commit(self)
            }
            fn rollback(&mut self) -> ::lightshade::Result<()> {
                App::rollback(self)
            }
            fn in_transaction(&self) -> bool {
                App::in_transaction(self)
            }
            fn transaction<T, F>(&mut self, f: F) -> ::lightshade::Result<T>
            where F: ::std::ops::FnOnce(&mut Self) -> ::lightshade::Result<T>, Self: ::std::marker::Sized {
                App::transaction(self, f)
            }
            fn close(self) -> ::lightshade::Result<()> {
                App::close(self)
            }
        }
    }
}
