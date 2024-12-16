use proc_macro::TokenStream;
use quote::{quote, ToTokens};
use syn::{parse, Data, DeriveInput, Result};

pub fn expand(input: TokenStream) -> Result<TokenStream> {
    let input: DeriveInput = parse(input)?;
    let name = &input.ident;

    let expanded = match input.data {
        Data::Struct(data_struct) => {
            let fields: Vec<_> = data_struct
                .fields
                .iter()
                .filter(|f| {
                    f.attrs
                        .iter()
                        .find(|attr| attr.path().is_ident("skip_field"))
                        .is_none()
                })
                .map(|f| {
                    let field = f.ident.as_ref().map(|i| i.to_string()).unwrap_or_default();
                    let type_str = f.ty.to_token_stream().to_string();
                    quote! {
                        (#field, #type_str)
                    }
                })
                .collect::<Vec<_>>();
            quote! {
                impl #name {
                    pub fn fields() -> &'static[(&'static str, &'static str)] {
                        &[ #(#fields),* ]
                    }
                }
            }
        }
        Data::Enum(data_enum) => {
            let variants: Vec<_> = data_enum.variants.into_iter().map(|v| v.ident).collect();
            quote! {
                impl #name {
                    pub fn values() -> &'static[#name] {
                        &[ #(#name::#variants),* ]
                    }
                }
            }
        }
        Data::Union(data_union) => {
            let fields: Vec<_> = data_union
                .fields
                .named
                .iter()
                .filter(|f| {
                    f.attrs
                        .iter()
                        .find(|attr| attr.path().is_ident("skip_field"))
                        .is_none()
                })
                .map(|f| {
                    let field = f.ident.as_ref().map(|i| i.to_string()).unwrap_or_default();
                    let type_str = f.ty.to_token_stream().to_string();
                    quote! {
                        (#field, #type_str)
                    }
                })
                .collect::<Vec<_>>();
            quote! {
                impl #name {
                    pub fn fields() -> &'static[(&'static str, &'static str)] {
                        &[ #(#fields),* ]
                    }
                }
            }
        }
    };
    Ok(TokenStream::from(expanded))
}
