use proc_macro::TokenStream;
use quote::quote;

#[proc_macro_derive(FFIVec)]
pub fn ffi_vec(item: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(item as syn::DeriveInput);

    let name = &input.ident;

    let result = quote! {
        paste::item! {
            #[cfg(not(target_arch="wasm32"))]
            #[repr(C)]
            pub struct [<FFIVec_ #name>] {
                pub ptr: *mut *mut #name,
                pub len: usize,
                pub cap: usize,
            }

            #[cfg(not(target_arch="wasm32"))]
            impl Into<[<FFIVec_ #name>]> for Vec<#name> {
                fn into(self) -> [<FFIVec_ #name>] {
                    let list = self.iter().cloned().map(|item| Box::into_raw(Box::new(item))).collect::<Vec<_>>();
                    let mut vec = std::mem::ManuallyDrop::new(list);
                    let len = vec.len();
                    let cap = vec.capacity();
                    let ptr = vec.as_mut_ptr();
                    [<FFIVec_ #name>] { ptr, len, cap }
                }
            }

            #[cfg(not(target_arch="wasm32"))]
            impl Into<[<FFIVec_ #name>]> for &Vec<#name> {
                fn into(self) -> [<FFIVec_ #name>] {
                    let list = self.iter().cloned().map(|item| Box::into_raw(Box::new(item))).collect::<Vec<_>>();
                    let mut vec = std::mem::ManuallyDrop::new(list);
                    let len = vec.len();
                    let cap = vec.capacity();
                    let ptr = vec.as_mut_ptr();
                    [<FFIVec_ #name>] { ptr, len, cap }
                }
            }

            #[cfg(not(target_arch="wasm32"))]
            impl From<[<FFIVec_ #name>]> for Vec<#name> {
                fn from(item: [<FFIVec_ #name>]) -> Self {
                    unsafe {
                        let raw_list = Vec::from_raw_parts(item.ptr, item.len, item.cap);
                        let mut list = vec![];
                        for ptr in raw_list {
                            list.push(*Box::from_raw(ptr));
                        }
                        list
                    }
                }
            }

            #[cfg(not(target_arch="wasm32"))]
            impl From<Box<[<FFIVec_ #name>]>> for Vec<#name> {
                fn from(item: Box<[<FFIVec_ #name>]>) -> Self {
                    unsafe {
                        let raw_list = Vec::from_raw_parts(item.ptr, item.len, item.cap);
                        let mut list = vec![];
                        for ptr in raw_list {
                            list.push(*Box::from_raw(ptr));
                        }
                        list
                    }
                }
            }

            #[cfg(not(target_arch="wasm32"))]
            #[no_mangle]
            pub unsafe extern "C" fn [<ffivec_ #name:lower _free>](cvec: *mut [<FFIVec_ #name>]) {
                let raw_list = Box::from_raw(cvec);
                let list = Vec::<#name>::from(raw_list);
                drop(list)
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
            #[cfg(not(target_arch="wasm32"))]
            #[no_mangle]
            pub unsafe extern "C" fn [<#name:lower _free>](ptr: *mut #name) {
                if ptr.is_null() { return; }
                drop(Box::from_raw(ptr))
            }
        }
    };

    item_free.into()
}
