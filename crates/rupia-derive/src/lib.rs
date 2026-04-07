use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput, Expr, Fields, Lit, Meta, Token};

struct RupiaAttrs {
    format: Option<String>,
    min: Option<f64>,
    max: Option<f64>,
    min_length: Option<u64>,
    max_length: Option<u64>,
    pattern: Option<String>,
}

fn parse_rupia_attrs(attrs: &[syn::Attribute]) -> RupiaAttrs {
    let mut result = RupiaAttrs {
        format: None,
        min: None,
        max: None,
        min_length: None,
        max_length: None,
        pattern: None,
    };
    for attr in attrs {
        if !attr.path().is_ident("rupia") {
            continue;
        }
        let Ok(nested) =
            attr.parse_args_with(syn::punctuated::Punctuated::<Meta, Token![,]>::parse_terminated)
        else {
            continue;
        };
        for meta in &nested {
            let Meta::NameValue(nv) = meta else {
                continue;
            };
            let Expr::Lit(lit) = &nv.value else {
                continue;
            };
            let key = nv
                .path
                .get_ident()
                .map(ToString::to_string)
                .unwrap_or_default();
            match key.as_str() {
                "format" => {
                    if let Lit::Str(s) = &lit.lit {
                        result.format = Some(s.value());
                    }
                }
                "min" => match &lit.lit {
                    Lit::Float(f) => result.min = f.base10_parse().ok(),
                    Lit::Int(i) => result.min = i.base10_parse::<f64>().ok(),
                    _ => {}
                },
                "max" => match &lit.lit {
                    Lit::Float(f) => result.max = f.base10_parse().ok(),
                    Lit::Int(i) => result.max = i.base10_parse::<f64>().ok(),
                    _ => {}
                },
                "min_length" => {
                    if let Lit::Int(i) = &lit.lit {
                        result.min_length = i.base10_parse().ok();
                    }
                }
                "max_length" => {
                    if let Lit::Int(i) = &lit.lit {
                        result.max_length = i.base10_parse().ok();
                    }
                }
                "pattern" => {
                    if let Lit::Str(s) = &lit.lit {
                        result.pattern = Some(s.value());
                    }
                }
                _ => {}
            }
        }
    }
    result
}

fn build_field_patches(fields: &Fields) -> Vec<proc_macro2::TokenStream> {
    let Fields::Named(named) = fields else {
        return vec![];
    };
    let mut patches = vec![];
    for field in &named.named {
        let attrs = parse_rupia_attrs(&field.attrs);
        let has_any = attrs.format.is_some()
            || attrs.min.is_some()
            || attrs.max.is_some()
            || attrs.min_length.is_some()
            || attrs.max_length.is_some()
            || attrs.pattern.is_some();
        if !has_any {
            continue;
        }
        let field_name = field.ident.as_ref().unwrap().to_string();
        let mut stmts = vec![];
        if let Some(fmt) = &attrs.format {
            stmts.push(quote! {
                prop.insert("format".into(), serde_json::Value::String(#fmt.into()));
            });
        }
        if let Some(v) = attrs.min {
            stmts.push(quote! {
                prop.insert("minimum".into(), serde_json::json!(#v));
            });
        }
        if let Some(v) = attrs.max {
            stmts.push(quote! {
                prop.insert("maximum".into(), serde_json::json!(#v));
            });
        }
        if let Some(v) = attrs.min_length {
            stmts.push(quote! {
                prop.insert("minLength".into(), serde_json::json!(#v));
            });
        }
        if let Some(v) = attrs.max_length {
            stmts.push(quote! {
                prop.insert("maxLength".into(), serde_json::json!(#v));
            });
        }
        if let Some(pat) = &attrs.pattern {
            stmts.push(quote! {
                prop.insert("pattern".into(), serde_json::Value::String(#pat.into()));
            });
        }
        patches.push(quote! {
            if let Some(prop) = props.get_mut(#field_name).and_then(|v| v.as_object_mut()) {
                #(#stmts)*
            }
        });
    }
    patches
}

#[proc_macro_derive(Harness, attributes(rupia))]
pub fn derive_harness(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let patches = match &input.data {
        syn::Data::Struct(s) => build_field_patches(&s.fields),
        _ => vec![],
    };
    let patch_block = if patches.is_empty() {
        quote! {}
    } else {
        quote! {
            if let Some(props) = schema.get_mut("properties").and_then(|v| v.as_object_mut()) {
                #(#patches)*
            }
        }
    };
    let expanded = quote! {
        impl #impl_generics rupia_core::types::HasSchema for #name #ty_generics #where_clause {
            fn rupia_schema() -> serde_json::Value {
                let generator = schemars::r#gen::SchemaSettings::draft2019_09()
                    .with(|s| { s.inline_subschemas = false; })
                    .into_generator();
                let root = generator.into_root_schema_for::<#name>();
                let mut schema = serde_json::to_value(root).unwrap_or_default();
                #patch_block
                schema
            }
        }
    };
    TokenStream::from(expanded)
}
