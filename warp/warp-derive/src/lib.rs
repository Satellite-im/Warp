use proc_macro::TokenStream;
use quote::quote;

#[proc_macro_derive(FFIArray)]
pub fn ffi_array(item: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(item as syn::DeriveInput);

    let name = &input.ident;

    let result = quote! {
        paste::item! {
            #[cfg(not(target_arch="wasm32"))]
            #[no_mangle]
            pub unsafe extern "C" fn [<ffiarray_ #name:lower _get>](ptr: *const crate::ffi::FFIArray<#name>, index: usize) -> *mut #name {
                if ptr.is_null() {
                    return std::ptr::null_mut();
                }
                let array = &*(ptr);
                match array.get(index).cloned() {
                    Some(data) => Box::into_raw(Box::new(data)) as *mut #name,
                    None => std::ptr::null_mut(),
                }
            }

            #[cfg(not(target_arch="wasm32"))]
            #[no_mangle]
            pub unsafe extern "C" fn [<ffiarray_ #name:lower _length>](ptr: *const crate::ffi::FFIArray<#name>) -> usize {
                if ptr.is_null() {
                    return 0;
                }
                let array = &*(ptr);
                array.length()
            }

            #[cfg(not(target_arch="wasm32"))]
            #[no_mangle]
            pub unsafe extern "C" fn [<ffiarray_ #name:lower _free>](ptr: *mut crate::ffi::FFIArray<#name>) {
                if ptr.is_null() {
                    return;
                }
                drop(Box::from_raw(ptr))
            }

        }
    };

    result.into()
}

#[proc_macro_derive(FFIFree)]
pub fn ffi_free(item: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(item as syn::DeriveInput);

    let name = &input.ident;

    let item_free = quote! {
        paste::item! {
            #[no_mangle]
            pub unsafe extern "C" fn [<#name:lower _free>](ptr: *mut #name) {
                if ptr.is_null() { return; }
                drop(Box::from_raw(ptr))
            }
        }
    };

    item_free.into()
}