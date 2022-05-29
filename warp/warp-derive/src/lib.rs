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

    let result = quote! {
        paste::item! {
            #[no_mangle]
            pub unsafe extern "C" fn [<#name:lower _free>](ptr: *mut #name) {
                if ptr.is_null() { return; }
                drop(Box::from_raw(ptr))
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

        #[repr(C)]
        pub struct FFIResult<T> {
            pub data: *mut T,
            pub error: *mut ::libc::c_char,
        }

        impl<T> From<Result<T, crate::error::Error>> for FFIResult<T> {
            fn from(res: Result<T, crate::error::Error>) -> Self {
                match res {
                    Ok(t) => Self::ok(t),
                    Err(err) => Self::err(err),
                }
            }
        }

        impl<T> FFIResult<T> {
            pub fn ok(data: T) -> Self {
                Self {
                    data: Box::into_raw(Box::new(data)),
                    error: std::ptr::null_mut(),
                }
            }

            pub fn err(err: crate::error::Error) -> Self {
                Self{
                    data: std::ptr::null_mut(),
                    error: std::ffi::CString::new(err.to_string()).unwrap().into_raw(),
                }
            }
        }
    };

    ffi.into()
}
