extern crate proc_macro2;

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Data, DeriveInput};

/// Implements Display for enum values with formatting and snake_case conversion
/// This is used for CommunityPermissions
/// E.g.
/// ```
/// use enum_macro::EnumFormatting;
/// #[derive(EnumFormatting)]
/// pub enum SomeEnum {
///     #[permission(formatting="prefix.{}")]
///     SomeValue1,
///     SomeValue2
/// }
///
/// assert_eq!(SomeEnum::SomeValue1.to_string(), "prefix.some_value1");
/// assert_eq!(SomeEnum::SomeValue2.to_string(), "some_value2");
/// ```
#[proc_macro_derive(EnumFormatting, attributes(permission))]
pub fn permission_node(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    if let Data::Enum(enum_data) = &input.data {
        let mut variants = vec![];
        for variant in &enum_data.variants {
            let mut format = None;
            if let Some(attr) = variant
                .attrs
                .iter()
                .find(|attr| attr.path().is_ident("permission"))
            {
                if let syn::Meta::List(l) = &attr.meta {
                    if let Err(err) = l.parse_nested_meta(|meta| {
                        if meta.path.is_ident("formatting") {
                            let exp: syn::Expr = meta.value()?.parse()?;
                            if let syn::Expr::Lit(v) = exp {
                                if let syn::Lit::Str(s) = v.lit {
                                    format = Some(s.value());
                                }
                            }
                            return Ok(());
                        }
                        Err(meta.error("Unrecognized attribute"))
                    }) {
                        panic!("{:?}", err);
                    }
                }
            }
            let format = format.unwrap_or("{}".into());

            let variant_name = &variant.ident;

            variants.push(quote! {
                #name::#variant_name => {
                    let mut snake = String::new();
                    for (i, ch) in stringify!(#variant_name).char_indices() {
                        if i > 0 && ch.is_uppercase() {
                            snake.push('_');
                        }
                        snake.push(ch.to_ascii_lowercase());
                    }
                    format!(#format, snake)
                },
            });
        }

        let expanded = quote! {
            impl std::fmt::Display for #name {
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    let result = match self {
                        #(#variants)*
                    };
                    f.write_str(&format!("{}", result))
                }
            }
        };

        TokenStream::from(expanded)
    } else {
        panic!("Only Enums are supported");
    }
}

/// Implements #values() for enum values that returns all defined values in the enum
/// E.g.
/// ```
/// use enum_macro::EnumValues;
/// #[derive(EnumValues, PartialEq, Debug)]
/// pub enum SomeEnum {
///     SomeValue1,
///     SomeValue2
/// }
///
/// assert_eq!([SomeEnum::SomeValue1, SomeEnum::SomeValue2], SomeEnum::values());
/// ```
#[proc_macro_derive(EnumValues)]
pub fn enum_values(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    if let Data::Enum(enum_data) = input.data {
        let variants: Vec<_> = enum_data.variants.into_iter().map(|v| v.ident).collect();
        let expanded = quote! {
            impl #name {
                pub fn values() -> &'static[#name] {
                    &[ #(#name::#variants),* ]
                }
            }
        };
        TokenStream::from(expanded)
    } else {
        panic!("Only Enums are supported");
    }
}
