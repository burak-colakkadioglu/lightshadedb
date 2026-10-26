use crate::model::{type_is, Operation, OperationKind, Schema};
use lightshade_codegen::ir::Ty;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::spanned::Spanned;
use syn::{Ident, Type};

pub(crate) fn typecheck_fn(operation: &Operation) -> TokenStream {
    let name = format_ident!("__ls_typecheck_{}", operation.method.sig.ident);
    let params = operation.method.sig.inputs.iter().filter_map(|input| {
        let syn::FnArg::Typed(typed) = input else {
            return None;
        };
        let pattern = &typed.pat;
        let ty = &typed.ty;
        Some(quote!(#pattern: #ty))
    });
    let mut expressions = Vec::new();
    match &operation.kind {
        OperationKind::Query(args) | OperationKind::Count(args) => {
            for join in &args.joins {
                expressions.push(quote!(#join));
            }
            if let Some(select) = &args.select {
                expressions.push(quote!(#select));
            }
            if let Some(filter) = &args.filter {
                expressions.push(quote!(#filter));
            }
            for group in &args.group_by {
                expressions.push(quote!(#group));
            }
            for order in &args.order {
                expressions.push(quote!(#order));
            }
            if let Some(limit) = &args.limit {
                expressions.push(quote!(#limit));
            }
            if let Some(offset) = &args.offset {
                expressions.push(quote!(#offset));
            }
        }
        OperationKind::Update(args) => {
            if let Some(filter) = &args.filter {
                expressions.push(quote!(#filter));
            }
            for set in &args.set {
                expressions.push(quote!(#set));
            }
        }
        OperationKind::Delete(args) => {
            if let Some(filter) = &args.filter {
                expressions.push(quote!(#filter));
            }
        }
    }
    quote! {
        #[doc(hidden)]
        #[allow(dead_code, non_snake_case, unused_variables)]
        fn #name(#(#params),*) {
            #(let _ = #expressions;)*
        }
    }
}

pub(crate) fn operation_impls(
    schema: &Schema,
    _target: &Ident,
    embedded: bool,
) -> Vec<TokenStream> {
    let Some(operations) = &schema.operations else {
        return Vec::new();
    };
    operations
        .methods
        .iter()
        .map(|operation| {
            let name = &operation.method.sig.ident;
            let arguments = operation
                .params
                .iter()
                .map(|param| {
                    let ident = &param.ident;
                    match param.ir {
                        Ty::Text => match &param.ty {
                            Type::Reference(_) => quote!(#ident.to_string()),
                            _ => quote!(#ident),
                        },
                        Ty::Bytes => match &param.ty {
                            Type::Reference(_) => quote!(#ident.to_vec()),
                            _ => quote!(#ident),
                        },
                        Ty::Uuid => match &param.ty {
                            Type::Reference(_) => quote!(*#ident),
                            _ => quote!(#ident),
                        },
                        _ => quote!(#ident),
                    }
                })
                .collect::<Vec<_>>();
            let params = operation.params.iter().map(|param| {
                let ident = &param.ident;
                let ty = &param.ty;
                quote!(#ident: #ty)
            });
            let receiver = match operation.kind {
                OperationKind::Query(_) | OperationKind::Count(_) => quote!(&self),
                _ => quote!(&mut self),
            };
            let output = &operation.method.sig.output;
            let body = match &operation.kind {
                OperationKind::Query(_) => quote! {
                    let rows = self.inner.#name(#(#arguments),*);
                    ::std::result::Result::Ok(rows)
                },
                OperationKind::Count(_) => quote! {
                    let value = self.inner.#name(#(#arguments),*);
                    ::std::result::Result::Ok(value)
                },
                OperationKind::Update(_) | OperationKind::Delete(_) => quote! {
                    let value = __ls_public(self.inner.#name(#(#arguments),*))?;
                    ::std::result::Result::Ok(value)
                },
            };
            let _ = embedded;
            quote! {
                pub fn #name(#receiver, #(#params),*) #output {
                    #body
                }
            }
        })
        .collect()
}

pub(crate) fn operation_trait_impls(schema: &Schema, target: &Ident) -> TokenStream {
    let Some(operations) = &schema.operations else {
        return TokenStream::new();
    };
    let trait_ident = &operations.ident;
    let methods = operations.methods.iter().map(|operation| {
        let sig = &operation.method.sig;
        let name = &sig.ident;
        let args = operation
            .params
            .iter()
            .map(|param| &param.ident)
            .collect::<Vec<_>>();
        quote! {
            #sig {
                #target::#name(self, #(#args),*)
            }
        }
    });
    quote! {
        impl #trait_ident for #target {
            #(#methods)*
        }
    }
}

pub(crate) enum OperationReturn {
    Rows(Ident, bool),
    Count,
    Affected,
}

pub(crate) fn operation_return(
    operation: &Operation,
    schema: &Schema,
) -> syn::Result<OperationReturn> {
    let syn::ReturnType::Type(_, ty) = &operation.method.sig.output else {
        return Err(syn::Error::new(
            operation.method.sig.output.span(),
            "operation needs a Result return type",
        ));
    };
    let Type::Path(path) = ty.as_ref() else {
        return Err(syn::Error::new(
            ty.span(),
            "operation needs a Result return type",
        ));
    };
    let result = path.path.segments.last().unwrap();
    let syn::PathArguments::AngleBracketed(arguments) = &result.arguments else {
        return Err(syn::Error::new(
            ty.span(),
            "operation needs a Result return type",
        ));
    };
    let Some(syn::GenericArgument::Type(inner)) = arguments.args.first() else {
        return Err(syn::Error::new(
            ty.span(),
            "operation needs a Result return type",
        ));
    };
    if let Type::Path(inner_path) = inner {
        if let Some(vec) = inner_path.path.segments.last() {
            if vec.ident == "Vec" {
                if let syn::PathArguments::AngleBracketed(items) = &vec.arguments {
                    if let Some(syn::GenericArgument::Type(Type::Path(row))) = items.args.first() {
                        let ident = row.path.segments.last().unwrap().ident.clone();
                        let projection = schema.projections.iter().any(|p| p.ident == ident);
                        return Ok(OperationReturn::Rows(ident, projection));
                    }
                }
            }
        }
    }
    if type_is(inner, "i64") {
        return Ok(OperationReturn::Count);
    }
    if type_is(inner, "u64") {
        return Ok(OperationReturn::Affected);
    }
    Err(syn::Error::new(
        inner.span(),
        "operation return must be Result<Vec<Row>>, Result<i64>, or Result<u64>",
    ))
}

pub(crate) fn row_trait(projection: bool) -> TokenStream {
    if projection {
        quote!(::lightshade::Projection)
    } else {
        quote!(::lightshade::Table)
    }
}
