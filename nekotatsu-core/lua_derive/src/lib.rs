// Note: currently unused but will likely be used in the future

use proc_macro2::TokenStream;
use quote::{quote, quote_spanned};
use syn::{self, Data, DeriveInput, Fields, parse_macro_input, spanned::Spanned};

/// *Super* basic `IntoLua` implementation for structs
/// that can be effectively treated as tables.
/// Treats structs with anonymous fields as tables
/// with number indices starting from 1.
#[proc_macro_derive(IntoLua)]
pub fn into_lua(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let name = input.ident;
    let data = &input.data;

    let add_fields = append_fields(data);

    let expanded = quote! {
        impl mlua::IntoLua for #name {
            fn into_lua(self, lua: &mlua::Lua) -> mlua::Result<mlua::Value> {
                let table = lua.create_table()?;
                #add_fields

                Ok(mlua::Value::Table(table))
            }
        }
    };

    proc_macro::TokenStream::from(expanded)
}

fn append_fields(data: &Data) -> TokenStream {
    match *data {
        Data::Struct(ref data) => match data.fields {
            Fields::Named(ref fields) => {
                let recurse = fields.named.iter().map(|f| {
                    let name = &f.ident.as_ref();
                    let stringed = name
                        .map(|i| i.clone().to_string())
                        .expect("field should have name");
                    quote_spanned! {f.span()=>
                        table.set(#stringed, self.#name)?;
                    }
                });

                quote! {#(#recurse)*}
            }
            Fields::Unnamed(ref fields) => {
                let recurse = fields.unnamed.iter().enumerate().map(|(i, f)| {
                    let index = syn::Index::from(i);
                    quote_spanned! {f.span()=>
                        table.set(#i + 1, self.#index)?;
                    }
                });
                quote! {#(#recurse)*}
            }
            _ => quote!(0),
        },
        Data::Enum(_) | Data::Union(_) => unimplemented!(),
    }
}
