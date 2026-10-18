extern crate proc_macro;
use proc_macro::TokenStream;
use quote::quote;
use syn::LitStr;
use syn::parse_macro_input;

#[proc_macro]
pub fn sc(input: TokenStream) -> TokenStream {
  let input_lit = parse_macro_input!(input as LitStr);
  let s = input_lit.value();

  // Check each character
  for c in s.chars() {
    if !c.is_ascii() {
      let msg = format!(
        "Unsupported character: '{}'. Only ASCII characters are known to have a Unicode width of 1. Don't use this macro in this case.",
        c
      );
      return syn::Error::new(input_lit.span(), msg).to_compile_error().into();
    }
  }

  let char_count = s.len() as u32;

  TokenStream::from(quote! {
    {
      const STRING_CONTAINER: dprint_core::formatting::StringContainer = dprint_core::formatting::StringContainer::proc_macro_new_with_char_count(#s, #char_count);
      &STRING_CONTAINER
    }
  })
}
