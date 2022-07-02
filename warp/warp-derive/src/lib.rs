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
pub fn construct_ffi(_: TokenStream) -> TokenStream {
    let ffi = quote! {
        pub struct FFIArray<T> {
            value: Vec<T>,
        }

        #[repr(C)]
        pub struct FFIVec<T> {
            pub ptr: *mut T,
            pub len: usize,
            pub cap: usize,
        }

        impl<T> FFIVec<T> {
            pub fn from(vec: Vec<T>) -> Self {
                let mut vec = std::mem::ManuallyDrop::new(vec);
                let len = vec.len();
                let cap = vec.capacity();
                let ptr = vec.as_mut_ptr();
                Self { ptr, len, cap }
            }

            pub fn into_vec(mut self) -> Vec<T> {
                unsafe { Vec::from_raw_parts(self.ptr, self.len, self.cap) }
            }
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
        pub struct FFIError {
            pub error_type: *mut std::os::raw::c_char,
            pub error_message: *mut std::os::raw::c_char,
        }

        impl FFIError {
            pub fn new(err: crate::error::Error) -> Self {
                let error_message = std::ffi::CString::new(err.to_string()).unwrap().into_raw();
                let error_type = std::ffi::CString::new(err.enum_to_string()).unwrap().into_raw();

                FFIError {
                    error_type,
                    error_message,
                }
            }

            pub fn to_ptr(self) -> *mut FFIError {
                Box::into_raw(Box::new(self))
            }

            pub fn from_ptr(ptr: *mut FFIError) -> (String, String) {
                let mut error = unsafe { Box::from_raw(ptr) };
                let error_type = match error.error_type.is_null() {
                    false => unsafe { std::ffi::CString::from_raw(error.error_type).into_string().unwrap() },
                    true => String::new()
                };
                let error_message = match error.error_message.is_null() {
                    false => unsafe { std::ffi::CString::from_raw(error.error_message).into_string().unwrap() },
                    true => String::new()
                };

                (error_type, error_message)
            }
        }

        #[repr(C)]
        pub struct FFIResult<T> {
            pub data: *mut T,
            pub error: *mut FFIError,
        }

        #[repr(C)]
        pub struct FFIResult_String {
            pub data: *mut std::os::raw::c_char,
            pub error: *mut FFIError,
        }

        impl From<Result<String, crate::error::Error>> for FFIResult_String {
            fn from(res: Result<String, crate::error::Error>) -> Self {
                match res {
                    Ok(t) => {
                        let data = std::ffi::CString::new(t).unwrap().into_raw();
                        Self {
                            data,
                            error: std::ptr::null_mut(),
                        }
                    }
                    Err(err) => {
                        let error = FFIError::new(err).to_ptr();
                        Self {
                            data: std::ptr::null_mut(),
                            error,
                        }
                    },
                }
            }
        }

        impl<T> From<Result<Vec<T>, crate::error::Error>> for FFIResult<FFIVec<T>> {
            fn from(res: Result<Vec<T>, crate::error::Error>) -> Self {
                match res {
                    Ok(t) => {
                        let data = Box::into_raw(Box::new(FFIVec::from(t)));
                        Self {
                            data,
                            error: std::ptr::null_mut(),
                        }
                    }
                    Err(err) => Self::err(err),
                }
            }
        }

        impl From<Result<String, crate::error::Error>> for FFIResult<std::os::raw::c_char> {
            fn from(res: Result<String, crate::error::Error>) -> Self {
                match res {
                    Ok(t) => {
                        let data = std::ffi::CString::new(t).unwrap().into_raw();
                        Self {
                            data,
                            error: std::ptr::null_mut(),
                        }
                    }
                    Err(err) => Self::err(err),
                }
            }
        }

        impl From<Result<(), crate::error::Error>> for FFIResult<std::os::raw::c_void> {
            fn from(res: Result<(), crate::error::Error>) -> Self {
                match res {
                    Ok(_) => Self {
                        data: std::ptr::null_mut(),
                        error: std::ptr::null_mut(),
                    },
                    Err(err) => Self::err(err),
                }
            }
        }

        impl<T> FFIResult<T> {
            /// Convert a Result<T, warp::error::Error> into FFIResult
            pub fn import(result: std::result::Result<T, crate::error::Error>) -> Self {
                match result {
                    Ok(t) => FFIResult::ok(t),
                    Err(err) => FFIResult::err(err),
                }
            }

            /// Produce a successful FFIResult
            pub fn ok(data: T) -> Self {
                Self {
                    data: Box::into_raw(Box::new(data)),
                    error: std::ptr::null_mut(),
                }
            }

            /// Produce a error of FFIResult
            pub fn err(err: crate::error::Error) -> Self {
                let error = FFIError::new(err).to_ptr();
                Self {
                    data: std::ptr::null_mut(),
                    error,
                }
            }
        }

        #[no_mangle]
        pub unsafe extern "C" fn ffierror_free(ptr: *mut FFIError) {
            if ptr.is_null() {
                return;
            }

            let _ = FFIError::from_ptr(ptr);
        }
    };

    ffi.into()
}
