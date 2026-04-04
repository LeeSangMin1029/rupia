use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput};

#[proc_macro_derive(Harness)]
pub fn derive_harness(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let expanded = quote! {
        impl #impl_generics rupia_core::types::HasSchema for #name #ty_generics #where_clause {
            fn json_schema() -> serde_json::Value {
                let gen = schemars::gen::SchemaSettings::draft2019_09()
                    .with(|s| { s.inline_subschemas = false; })
                    .into_generator();
                let schema = gen.into_root_schema_for::<#name>();
                serde_json::to_value(schema).unwrap_or_default()
            }
        }
    };
    TokenStream::from(expanded)
}
