use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{
    parenthesized,
    parse::{Parse, ParseStream},
    parse_macro_input,
    punctuated::Punctuated,
    Ident, Result, Token, TypePath, Visibility,
};

struct CallbackTypes {
    pub interface: TypePath,
    pub arg_1: TypePath,
    pub arg_2: TypePath,
}

impl Parse for CallbackTypes {
    fn parse(input: ParseStream) -> Result<Self> {
        let content;
        parenthesized!(content in input);
        let args: Punctuated<TypePath, Token![,]> = content.parse_terminated(TypePath::parse)?;
        input.parse::<Token![;]>()?;
        if args.len() == 3 {
            let mut args = args.into_iter();

            Ok(CallbackTypes {
                interface: args.next().unwrap(),
                arg_1: args.next().unwrap(),
                arg_2: args.next().unwrap(),
            })
        } else {
            Err(content.error("expected (interface, arg_1, arg_2)"))
        }
    }
}

struct CallbackStruct {
    pub vis: Visibility,
    pub struct_token: Token![struct],
    pub ident: Ident,
    pub args: CallbackTypes,
}

impl Parse for CallbackStruct {
    fn parse(input: ParseStream) -> Result<Self> {
        Ok(CallbackStruct {
            vis: input.parse()?,
            struct_token: input.parse()?,
            ident: input.parse()?,
            args: input.parse()?,
        })
    }
}

#[proc_macro_attribute]
pub fn completed_callback(_attr: TokenStream, input: TokenStream) -> TokenStream {
    let ast = parse_macro_input!(input as CallbackStruct);
    impl_completed_callback(&ast).expect("error in impl_completed_callback")
}

fn impl_completed_callback(ast: &CallbackStruct) -> Result<TokenStream> {
    let vis = &ast.vis;

    let name = &ast.ident;
    let closure = get_closure(name);

    let interface = &ast.args.interface;
    let abi = get_abi(interface);

    let arg_1 = &ast.args.arg_1;
    let arg_2 = &ast.args.arg_2;

    let gen = quote! {
        type #closure<'a> = CompletedClosure<'a, #arg_1, #arg_2>;

        #[repr(C)]
        #vis struct #name<'a> {
            vtable: *const #abi,
            refcount: AtomicU32,
            completed: Option<#closure<'a>>,
        }

        impl<'a> Callback<'a> for #name<'a> {
            type Interface = #interface;
            type Closure = #closure<'a>;

            fn new(completed: #closure<'a>) -> Self {
                static VTABLE: #abi = #abi(
                    #name::query_interface,
                    #name::add_ref,
                    #name::release,
                    #name::invoke,
                );

                Self {
                    vtable: &VTABLE,
                    refcount: AtomicU32::new(1),
                    completed: Some(completed),
                }
            }
        }

        impl<'a> CallbackInterface<'a, #name<'a>> for #name<'a> {
            fn refcount(&self) -> &AtomicU32 {
                &self.refcount
            }
        }

        impl<'a> CompletedCallback<'a, #name<'a>, #arg_1, #arg_2> for #name<'a> {
            fn completed(&mut self) -> Option<#closure<'a>> {
                self.completed.take()
            }
        }
    };

    Ok(gen.into())
}

#[proc_macro_attribute]
pub fn event_callback(_attr: TokenStream, input: TokenStream) -> TokenStream {
    let ast = parse_macro_input!(input as CallbackStruct);
    impl_event_callback(&ast).expect("error in impl_event_callback")
}

fn impl_event_callback(ast: &CallbackStruct) -> Result<TokenStream> {
    let vis = &ast.vis;

    let name = &ast.ident;
    let closure = get_closure(name);

    let interface = &ast.args.interface;
    let abi = get_abi(interface);

    let arg_1 = &ast.args.arg_1;
    let arg_2 = &ast.args.arg_2;

    let gen = quote! {
        type #closure<'a> = EventClosure<'a, #arg_1, #arg_2>;

        #[repr(C)]
        #vis struct #name<'a> {
            vtable: *const #abi,
            refcount: AtomicU32,
            event: #closure<'a>,
        }

        impl<'a> Callback<'a> for #name<'a> {
            type Interface = #interface;
            type Closure = #closure<'a>;

            fn new(event: #closure<'a>) -> Self {
                static VTABLE: #abi = #abi(
                    #name::query_interface,
                    #name::add_ref,
                    #name::release,
                    #name::invoke,
                );

                Self {
                    vtable: &VTABLE,
                    refcount: AtomicU32::new(1),
                    event,
                }
            }
        }

        impl<'a> CallbackInterface<'a, #name<'a>> for #name<'a> {
            fn refcount(&self) -> &AtomicU32 {
                &self.refcount
            }
        }

        impl<'a> EventCallback<'a, #name<'a>, #arg_1, #arg_2> for #name<'a> {
            fn event(&mut self) -> &mut #closure<'a> {
                &mut self.event
            }
        }
    };

    Ok(gen.into())
}

fn get_closure(name: &Ident) -> Ident {
    format_ident!("{}Closure", name)
}

fn get_abi(interface: &TypePath) -> TypePath {
    let mut abi = interface.clone();
    let last_ident = &mut abi
        .path
        .segments
        .last_mut()
        .expect("closure.path.segments.last_mut()")
        .ident;
    *last_ident = format_ident!("{}_abi", last_ident);

    abi
}
