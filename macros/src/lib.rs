use proc_macro::TokenStream;
use syn::parse_macro_input;

mod expand;
mod lower;
mod model;
mod registry;

#[proc_macro_attribute]
pub fn schema(_args: TokenStream, input: TokenStream) -> TokenStream {
    let module = parse_macro_input!(input as syn::ItemMod);
    match model::parse_schema(module).and_then(expand::expand) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

#[proc_macro_attribute]
pub fn operations(_args: TokenStream, input: TokenStream) -> TokenStream {
    input
}

#[proc_macro]
pub fn registry(input: TokenStream) -> TokenStream {
    let tokens = proc_macro2::TokenStream::from(input);
    match registry::registry(tokens) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}
