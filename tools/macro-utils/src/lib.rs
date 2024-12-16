extern crate proc_macro2;

use proc_macro::TokenStream;

mod field_utils;
mod struct_utils;

/// Implements either:
///     #values() -> &'static[#Enum] for enums that returns all defined values in the enum.
///     #fields() -> &'static[(field_name, field_type_definition)] for struct and union types that returns all defined fields.
/// E.g.
/// ```
/// use macro_utils::VariantExport;
/// #[derive(VariantExport, PartialEq, Debug)]
/// pub enum SomeEnum {
///     SomeValue1,
///     SomeValue2
/// }
///
/// assert_eq!([SomeEnum::SomeValue1, SomeEnum::SomeValue2], SomeEnum::values());
/// 
/// #[derive(VariantExport, PartialEq, Debug)]
/// pub struct B;
/// 
/// pub struct A {
///     x: i64,
///     y: i128,
///     z: B
/// }
///
/// assert_eq!([("x", "i64"), ("y", "i128"), ("z", "B")], A::fields());
/// ```
#[proc_macro_derive(VariantExport)]
pub fn field_values(input: TokenStream) -> TokenStream {
    field_utils::expand(input)
        .unwrap_or_else(|e| e.into_compile_error().into())
        .into()
}

/// Implements a way to fetch all public implemented method for a given struct.
/// It returns a tuple where the first element is the functions name and the second if its async
/// E.g.
/// ```
/// use macro_utils::impl_funcs;
/// 
/// pub struct A;
/// 
/// #[impl_funcs(name="get_all_impls")] /// Optional name parameter. If not present will be called functions
/// impl A {
///     fn some_method_a() {}
/// 
///     fn some_method_b() {}
/// 
///     async fn some_async_method_c() {}
/// }
/// assert_eq!([("some_method_a", false), ("some_method_b", false), ("some_async_method_c", true)], A::get_all_impls());
/// ```
#[proc_macro_attribute]
pub fn impl_funcs(attr: TokenStream, input: TokenStream) -> TokenStream {
    struct_utils::expand(attr, input)
        .unwrap_or_else(|e| e.into_compile_error().into())
        .into()
}