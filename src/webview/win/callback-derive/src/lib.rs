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
  impl_completed_callback(&ast)
}

fn impl_completed_callback(ast: &CallbackStruct) -> TokenStream {
  let vis = &ast.vis;

  let name = &ast.ident;
  let closure = get_closure(name);

  let interface = &ast.args.interface;
  let abi = get_abi(interface);

  let arg_1 = &ast.args.arg_1;
  let arg_2 = &ast.args.arg_2;

  let gen = quote! {
      use windows as _;

      type #closure<'a> = CompletedClosure<'a, #arg_1, #arg_2>;

      #[repr(C)]
      #vis struct #name<'a> {
          vtable: *const #abi,
          refcount: AtomicU32,
          completed: Option<#closure<'a>>,
      }

      impl<'a> #name<'a> {
          pub fn create(completed: #closure<'a>) -> windows::Result<#interface> {
              let handler = Box::new(Self::new(completed));
              let handler = unsafe { Self::from_abi(Box::into_raw(handler) as windows::RawPtr)? };
              Ok(handler)
          }

          unsafe fn from_abi(this: windows::RawPtr) -> windows::Result<#interface> {
              let unknown = windows::IUnknown::from_abi(this)?;
              unknown.vtable().1(unknown.abi()); // add_ref to balance the release called in IUnknown::drop
              unknown.cast()
          }

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

          /// The WebView2 threading model runs everything on the UI thread, including callbacks which it triggers
          /// with `PostMessage`, and we're using this here because it's waiting for some async operations in WebView2
          /// to finish before starting the main message loop in `EventLoop::run`. As long as there are no pending
          /// results in `rx`, it will poll the [`EventLoop`] with [`EventLoopExtRunReturn::run_return`] and check for a
          /// result after each message is dispatched.
          pub fn wait_for_async_operation(
              closure: Box<dyn FnOnce(<Self as Callback<'a>>::Interface) -> windows::HRESULT>,
              completed: <Self as Callback<'a>>::Closure,
          ) -> super::Result<()> {
              let (tx, rx) = mpsc::channel();
              let completed = Box::new(move |arg_1, arg_2| {
                  tx.send(completed(arg_1, arg_2))
                      .expect("send over mpsc channel");
                  S_OK
              });
              let callback = <Self as Callback<'a>>::create(completed)?;

              let error_code = closure(callback);
              if error_code.is_err() {
                  return Err(windows::Error::fast_error(error_code).into());
              }

              let mut result = Err(RecvError.into());
              let mut event_loop = EventLoop::new();
              event_loop.run_return(|_, _, control_flow| {
                  if let Ok(value) = rx.try_recv() {
                      *control_flow = ControlFlow::Exit;
                      if value.is_ok() {
                          result = Ok(());
                      } else {
                          result = Err(windows::Error::fast_error(value).into());
                      }
                  } else {
                      *control_flow = ControlFlow::Poll;
                  }
              });

              result
          }
      }

      impl<'a> Callback<'a> for #name<'a> {
          type Interface = #interface;
          type Closure = #closure<'a>;

          fn create(completed: #closure<'a>) -> windows::Result<#interface> {
            Self::create(completed)
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

  gen.into()
}

#[proc_macro_attribute]
pub fn event_callback(_attr: TokenStream, input: TokenStream) -> TokenStream {
  let ast = parse_macro_input!(input as CallbackStruct);
  impl_event_callback(&ast)
}

fn impl_event_callback(ast: &CallbackStruct) -> TokenStream {
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

      impl<'a> #name<'a> {
        pub fn create(event: #closure<'a>) -> windows::Result<#interface> {
            let handler = Box::new(Self::new(event));
            let handler = unsafe { Self::from_abi(Box::into_raw(handler) as windows::RawPtr)? };
            Ok(handler)
        }

        unsafe fn from_abi(this: windows::RawPtr) -> windows::Result<#interface> {
              let unknown = windows::IUnknown::from_abi(this)?;
              unknown.vtable().1(unknown.abi()); // add_ref to balance the release called in IUnknown::drop
              unknown.cast()
          }

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

      impl<'a> Callback<'a> for #name<'a> {
          type Interface = #interface;
          type Closure = #closure<'a>;

          fn create(event: #closure<'a>) -> windows::Result<#interface> {
              Self::create(event)
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

  gen.into()
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
    .expect("abi.path.segments.last_mut()")
    .ident;
  *last_ident = format_ident!("{}_abi", last_ident);

  abi
}
