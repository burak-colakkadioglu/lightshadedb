use lightshade_codegen::registry::{
    field_name, parse_registry, split_listen, DatabaseEntry, Registry, RegistryMode,
};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

pub fn registry(input: TokenStream) -> syn::Result<TokenStream> {
    let path: syn::LitStr = syn::parse2(input)?;
    let manifest = std::env::var("CARGO_MANIFEST_DIR").map_err(|_| {
        syn::Error::new(
            path.span(),
            "cannot locate Cargo.toml: `CARGO_MANIFEST_DIR` is not set",
        )
    })?;
    let file = std::path::Path::new(&manifest).join(path.value());
    let source = std::fs::read_to_string(&file).map_err(|error| {
        syn::Error::new(
            path.span(),
            format!("cannot read `{}`: {error}", file.display()),
        )
    })?;
    let registry =
        parse_registry(&source).map_err(|message| syn::Error::new(path.span(), message))?;
    expand_registry(&registry)
}

fn schema_path(entry: &DatabaseEntry) -> syn::Result<syn::Path> {
    syn::parse_str::<syn::Path>(&entry.schema).map_err(|_| {
        syn::Error::new(
            proc_macro2::Span::call_site(),
            format!(
                "database `{}` has an invalid schema path `{}`",
                entry.id, entry.schema
            ),
        )
    })
}

fn entry_field(entry: &DatabaseEntry) -> syn::Result<TokenStream> {
    let field = format_ident!("{}", field_name(&entry.id));
    let schema = schema_path(entry)?;
    let ty = match entry.mode {
        RegistryMode::Embedded | RegistryMode::Client => quote!(#schema::App),
        RegistryMode::Host => quote!(#schema::Server),
    };
    let gate = feature_gate(entry);
    Ok(quote! {
        #gate
        pub #field: #ty,
    })
}

fn feature_gate(entry: &DatabaseEntry) -> TokenStream {
    let feature = entry.mode.feature();
    quote!(#[cfg(feature = #feature)])
}

fn open_entry(entry: &DatabaseEntry) -> syn::Result<TokenStream> {
    let field = format_ident!("{}", field_name(&entry.id));
    let schema = schema_path(entry)?;
    let gate = feature_gate(entry);
    let body = match entry.mode {
        RegistryMode::Embedded => {
            let path = entry.path.clone().unwrap_or_default();
            quote!(#schema::App::open(#schema::AppConfig::embedded(#path))?)
        }
        RegistryMode::Host => {
            let id = entry.id.clone();
            let path = entry.path.clone().unwrap_or_default();
            let listen = entry.listen.clone().unwrap_or_default();
            let access = entry.access.clone().unwrap_or_else(|| "local".to_string());
            let allow = entry.allow.clone();
            let credentials = entry.credentials_file.clone().unwrap_or_default();
            quote! {
                #schema::Server::start_all(
                    ::std::vec![#schema::ServedDb {
                        name: #id.into(),
                        db: #schema::Db::open(#path)?,
                    }],
                    {
                        let mut options = ::lightshade::ServeOptions::new(
                            ::lightshade::CredentialsSource::File(#credentials.into()),
                        );
                        options.listen = ::lightshade::Listen::parse_tcp(#listen, #access.parse()?)?;
                        options.listen = options.listen.with_allowlist(&[#(#allow,)*])?;
                        options
                    },
                )?
            }
        }
        RegistryMode::Client => {
            let endpoint = entry.endpoint.clone().unwrap_or_default();
            let id = entry.credential_id.clone().unwrap_or_default();
            let secret = secret_var(&entry.id);
            let secret_missing = format!(
                "set {secret} to the credential secret for database `{}`",
                entry.id
            );
            quote! {
                #schema::App::open(#schema::AppConfig::client(
                    ::lightshade::ClientEndpoint::parse(#endpoint)?,
                    ::lightshade::ClientOptions::new(::lightshade::ClientCredentials::new(
                        #id,
                        ::std::env::var(#secret).map_err(|_| ::lightshade::Error::InvalidArgument {
                            name: #secret.into(),
                            reason: #secret_missing.into(),
                        })?,
                    )),
                ))?
            }
        }
    };
    Ok(quote! {
        #gate
        #field: { #body },
    })
}

fn from_env_entry(entry: &DatabaseEntry) -> syn::Result<TokenStream> {
    let field = format_ident!("{}", field_name(&entry.id));
    let schema = schema_path(entry)?;
    let gate = feature_gate(entry);
    let prefix = Registry::env_prefix(&entry.id);
    let body = match entry.mode {
        RegistryMode::Embedded => {
            let fallback = entry.path.clone().unwrap_or_default();
            let var = format!("{prefix}_PATH");
            quote! {
                #schema::App::open(#schema::AppConfig::embedded(
                    ::std::env::var(#var).unwrap_or_else(|_| #fallback.to_string()),
                ))?
            }
        }
        RegistryMode::Host => {
            let id = entry.id.clone();
            let fallback_path = entry.path.clone().unwrap_or_default();
            let fallback_listen = entry.listen.clone().unwrap_or_default();
            let fallback_creds = entry.credentials_file.clone().unwrap_or_default();
            let fallback_allow = entry.allow.clone().join(",");
            let access = entry.access.clone().unwrap_or_else(|| "local".to_string());
            let path_var = format!("{prefix}_PATH");
            let listen_var = format!("{prefix}_LISTEN");
            let creds_var = format!("{prefix}_CREDENTIALS");
            let allow_var = format!("{prefix}_ALLOW");
            quote! {
                #schema::Server::start_all(
                    ::std::vec![#schema::ServedDb {
                        name: #id.into(),
                        db: #schema::Db::open(
                            ::std::env::var(#path_var).unwrap_or_else(|_| #fallback_path.to_string()),
                        )?,
                    }],
                    {
                        let mut options = ::lightshade::ServeOptions::new(
                            ::lightshade::CredentialsSource::File(
                                ::std::env::var(#creds_var)
                                    .unwrap_or_else(|_| #fallback_creds.to_string())
                                    .into(),
                            ),
                        );
                        let listen = ::std::env::var(#listen_var).unwrap_or_else(|_| #fallback_listen.to_string());
                        let allow = ::std::env::var(#allow_var).unwrap_or_else(|_| #fallback_allow.to_string());
                        let allow: ::std::vec::Vec<&str> = allow.split(',').filter(|entry| !entry.trim().is_empty()).collect();
                        options.listen = ::lightshade::Listen::parse_tcp(&listen, #access.parse()?)?;
                        options.listen = options.listen.with_allowlist(&allow)?;
                        options
                    },
                )?
            }
        }
        RegistryMode::Client => {
            let fallback = entry.endpoint.clone().unwrap_or_default();
            let id = entry.credential_id.clone().unwrap_or_default();
            let endpoint_var = format!("{prefix}_ENDPOINT");
            let secret_var = secret_var(&entry.id);
            let secret_missing = format!(
                "set {secret_var} to the credential secret for database `{}`",
                entry.id
            );
            quote! {
                #schema::App::open(#schema::AppConfig::client(
                    ::lightshade::ClientEndpoint::parse(
                        &::std::env::var(#endpoint_var).unwrap_or_else(|_| #fallback.to_string()),
                    )?,
                    ::lightshade::ClientOptions::new(::lightshade::ClientCredentials::new(
                        #id,
                        ::std::env::var(#secret_var).map_err(|_| ::lightshade::Error::InvalidArgument {
                            name: #secret_var.into(),
                            reason: #secret_missing.into(),
                        })?,
                    )),
                ))?
            }
        }
    };
    Ok(quote! {
        #gate
        #field: { #body },
    })
}

fn secret_var(id: &str) -> String {
    format!("{}_SECRET", Registry::env_prefix(id))
}

fn close_entry(entry: &DatabaseEntry) -> syn::Result<TokenStream> {
    let field = format_ident!("{}", field_name(&entry.id));
    let gate = feature_gate(entry);
    let close = match entry.mode {
        RegistryMode::Host => quote!(self.#field.join()?;),
        _ => quote!(self.#field.close()?;),
    };
    Ok(quote! {
        #gate
        { #close }
    })
}

fn expand_registry(registry: &Registry) -> syn::Result<TokenStream> {
    for entry in &registry.databases {
        if entry.mode == RegistryMode::Host {
            split_listen(entry.listen.as_deref().unwrap_or(""))
                .map_err(|message| syn::Error::new(proc_macro2::Span::call_site(), message))?;
        }
        schema_path(entry)?;
    }
    let fields = registry
        .databases
        .iter()
        .map(entry_field)
        .collect::<syn::Result<Vec<_>>>()?;
    let opens = registry
        .databases
        .iter()
        .map(open_entry)
        .collect::<syn::Result<Vec<_>>>()?;
    let envs = registry
        .databases
        .iter()
        .map(from_env_entry)
        .collect::<syn::Result<Vec<_>>>()?;
    let closes = registry
        .databases
        .iter()
        .map(close_entry)
        .collect::<syn::Result<Vec<_>>>()?;
    let ids = registry.databases.iter().map(|entry| &entry.id);
    let requirements = registry.databases.iter().map(|entry| {
        let feature = entry.mode.feature();
        let id = &entry.id;
        let mode = match entry.mode {
            RegistryMode::Embedded => "embedded",
            RegistryMode::Host => "host",
            RegistryMode::Client => "client",
        };
        let message =
            format!("registry database `{id}` (mode `{mode}`) needs the `{feature}` Cargo feature");
        quote! {
            #[cfg(not(feature = #feature))]
            compile_error!(#message);
        }
    });
    Ok(quote! {
        #(#requirements)*

        pub struct Databases {
            #(#fields)*
        }

        pub const IDS: &[&str] = &[#(#ids,)*];

        impl Databases {
            pub fn open_all() -> ::lightshade::Result<Self> {
                Ok(Self {
                    #(#opens)*
                })
            }

            pub fn from_env_all() -> ::lightshade::Result<Self> {
                Ok(Self {
                    #(#envs)*
                })
            }

            pub fn close_all(self) -> ::lightshade::Result<()> {
                #(#closes)*
                ::std::result::Result::Ok(())
            }
        }
    })
}
