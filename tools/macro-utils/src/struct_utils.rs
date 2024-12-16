use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{
    parse::{Parse, Parser},
    punctuated::Punctuated,
    spanned::Spanned,
    Error, ImplItem, ItemImpl, Meta, Result, Type, Visibility,
};

pub fn expand(attr: TokenStream, input: TokenStream) -> Result<TokenStream> {
    let attributes = Punctuated::<Meta, syn::Token![,]>::parse_terminated.parse(attr)?;
    let impls: ItemImpl = ItemImpl::parse.parse(input)?;
    let ident;
    let mut _name = "functions".into();
    if let Type::Path(p) = impls.self_ty.as_ref() {
        ident = p.path.get_ident().unwrap();
    } else {
        return Err(Error::new(impls.span(), "Only applicable to impl blocks"));
    }
    for attr in attributes {
        if attr.path().is_ident("name") {
            if let syn::Meta::NameValue(val) = &attr {
                if let syn::Expr::Lit(v) = &val.value {
                    if let syn::Lit::Str(s) = &v.lit {
                        _name = s.value();
                    }
                }
            }
        } else {
            return Err(Error::new(
                attr.span(),
                format!(
                    "Unrecognized attribute {:?}",
                    attr.path()
                        .get_ident()
                        .map(|i| i.to_string())
                        .unwrap_or_default()
                ),
            ));
        }
    }

    let mut functions = vec![];
    for item in impls.items {
        if let ImplItem::Fn(function) = item {
            if (function
                .attrs
                .iter()
                .find(|attr| attr.path().is_ident("skip")))
            .is_none()
            {
                let is_async = function.sig.asyncness.is_some();
                if matches!(function.vis, Visibility::Public(_) | Visibility::Inherited) {
                    let id = function.sig.ident.to_string();
                    functions.push(quote! {
                        (#id, #is_async)
                    });
                }
            }
        }
    }
    let func = format_ident!("{_name}");
    let expanded = quote! {
        impl #ident {
            pub fn #func() -> &'static[(&'static str, bool)] {
                &[ #(#functions),* ]
            }
        }
    };
    Ok(TokenStream::from(expanded))
}
