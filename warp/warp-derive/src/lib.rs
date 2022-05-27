use proc_macro::TokenStream;
use quote::quote;

#[proc_macro_derive(FFIArray)]
pub fn ffi_array(item: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(item as syn::DeriveInput);

    let name = &input.ident;

    let result = quote! {
        paste::item! {
            #[no_mangle]
            pub extern "C" fn [<ffiarray_ #name:lower _get>](ptr: *const crate::ffi::FFIArray<#name>, index: usize) -> *mut #name {
                unsafe {
                    if ptr.is_null() {
                        return std::ptr::null_mut();
                    }
                    let array = &*(ptr);
                    match array.get(index).cloned() {
                        Some(data) => Box::into_raw(Box::new(data)) as *mut #name,
                        None => std::ptr::null_mut(),
                    }
                }
            }

            #[no_mangle]
            pub unsafe extern "C" fn [<ffiarray_ #name:lower _length>](ptr: *const crate::ffi::FFIArray<#name>) -> usize {
                if ptr.is_null() {
                    return 0;
                }
                let array = &*(ptr);
                array.length()
            }
        }
    };

    result.into()
}

#[proc_macro]
pub fn construct_ffi(_item: TokenStream) -> TokenStream {
    let ffi = quote! {
        pub struct FFIArray<T> {
            value: Vec<T>,
        }

        impl<T> FFIArray<T> {
            pub fn new(value: Vec<T>) -> FFIArray<T> {
                FFIArray { value }
            }

            pub fn get(&self, index: usize) -> Option<&T> {
                self.value.get(index)
            }
            pub fn length(&self) -> usize {
                self.value.len()
            }
        }
    };

    ffi.into()
}
