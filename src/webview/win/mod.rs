mod callback;
mod environment_options;
mod file_drop;

use bindings::{
  Microsoft::Web::WebView2 as webview2,
  Windows::Win32::{
    Com,
    DisplayDevices::RECT,
    Shell,
    SystemServices::PWSTR,
    WinRT::EventRegistrationToken,
    WindowsAndMessaging::{self, HWND},
  },
};

use crate::{
  webview::{mimetype::MimeType, WV},
  FileDropHandler, Result, RpcHandler,
};

use file_drop::FileDropController;

use std::{mem::{self, size_of}, path::PathBuf, ptr, rc::Rc, sync::mpsc::{self, RecvError}};

use once_cell::unsync::OnceCell;
use url::Url;
use winit::{
  event_loop::{ControlFlow, EventLoop},
  platform::{run_return::EventLoopExtRunReturn, windows::WindowExtWindows},
  window::Window,
};

pub struct InnerWebView {
  controller: Rc<OnceCell<webview2::ICoreWebView2Controller>>,
  webview: Rc<OnceCell<webview2::ICoreWebView2>>,

  // Store FileDropController in here to make sure it gets dropped when
  // the webview gets dropped, otherwise we'll have a memory leak
  #[allow(dead_code)]
  file_drop_controller: Rc<OnceCell<FileDropController>>,
}

impl WV for InnerWebView {
  type Window = Window;

  fn new<F: 'static + Fn(&str) -> Result<Vec<u8>>>(
    window: &Window,
    scripts: Vec<String>,
    url: Option<Url>,
    // TODO default background color option just adds to webview2 recently and it requires
    // canary build. Implement this once it's in official release.
    #[allow(unused_variables)] transparent: bool,
    custom_protocol: Option<(String, F)>,
    rpc_handler: Option<RpcHandler>,
    file_drop_handler: Option<FileDropHandler>,
    user_data_path: Option<PathBuf>,
  ) -> Result<Self> {
    let hwnd = HWND(window.hwnd() as _);

    let controller_rc: Rc<OnceCell<webview2::ICoreWebView2Controller>> = Rc::new(OnceCell::new());
    let webview_rc: Rc<OnceCell<webview2::ICoreWebView2>> = Rc::new(OnceCell::new());
    let file_drop_controller_rc: Rc<OnceCell<FileDropController>> = Rc::new(OnceCell::new());

    let env = unsafe {
      let mut result = None;

      wait_for_async_operation::<callback::CreateCoreWebView2EnvironmentCompletedHandler, callback::ErrorCodeArg, callback::InterfaceArg<webview2::ICoreWebView2Environment>>(
        Box::new(|environmentcreatedhandler: webview2::ICoreWebView2CreateCoreWebView2EnvironmentCompletedHandler| {
          match user_data_path {
            Some(user_data_path_provided) => webview2::CreateCoreWebView2EnvironmentWithOptions(
              "",
              user_data_path_provided.to_str().unwrap_or(""),
              environment_options::create_options().unwrap(),
              environmentcreatedhandler,
            ),
            None => webview2::CreateCoreWebView2Environment(environmentcreatedhandler),
          }
        }),
        Box::new(|error_code: windows::ErrorCode, environment: Option<webview2::ICoreWebView2Environment>| {
          if error_code.is_ok() {
            result = environment;
          }

          error_code
        }),
      )?;

      result.expect("async operation was successful")
    };

    // Webview controller
    let controller = unsafe {
      let mut result = None;
      let env_ = env.clone();

      wait_for_async_operation::<
        callback::CreateCoreWebView2ControllerCompletedHandler,
        callback::ErrorCodeArg,
        callback::InterfaceArg<webview2::ICoreWebView2Controller>,
      >(
        Box::new(
          move |handler: webview2::ICoreWebView2CreateCoreWebView2ControllerCompletedHandler| {
            env_.CreateCoreWebView2Controller(hwnd, handler)
          },
        ),
        Box::new(
          |error_code: windows::ErrorCode,
           controller: Option<webview2::ICoreWebView2Controller>| {
            if error_code.is_ok() {
              result = controller;
            }
            error_code
          },
        ),
      )?;

      result.expect("async operation was sucessful")
    };

    let w = unsafe {
      let mut result = None;
      assert!(controller.get_CoreWebView2(&mut result).is_ok());
      result.expect("operation was sucessful")
    };

    // Enable sensible defaults
    unsafe {
      let mut result = None;
      assert!(w.get_Settings(&mut result).is_ok());
      let settings = result.expect("operation was sucessful");

      assert!(settings.put_IsStatusBarEnabled(false).is_ok());
      assert!(settings.put_AreDefaultContextMenusEnabled(true).is_ok());
      assert!(settings.put_IsZoomControlEnabled(false).is_ok());
      assert!(settings.put_AreDevToolsEnabled(false).is_ok());
      debug_assert!(settings.put_AreDevToolsEnabled(true).is_ok());
    }

    // Safety: System calls are unsafe
    unsafe {
      let mut rect = RECT::default();
      WindowsAndMessaging::GetClientRect(hwnd, &mut rect);
      let (width, height) = (rect.right - rect.left, rect.bottom - rect.top);
      assert!(controller
        .put_Bounds(RECT {
          left: 0,
          top: 0,
          right: width,
          bottom: height,
        })
        .is_ok());
    }

    // Initialize scripts
    unsafe {
      let w_ = w.clone();

      wait_for_async_operation::<
        callback::AddScriptToExecuteOnDocumentCreatedCompletedHandler,
        callback::ErrorCodeArg,
        callback::StringArg,
      >(
        Box::new(move |handler| {
          w_.AddScriptToExecuteOnDocumentCreated(
            "window.external={invoke:s=>window.chrome.webview.postMessage(s)}",
            handler,
          )
        }),
        Box::new(|error_code: windows::ErrorCode, _id| error_code),
      )?;

      for js in scripts {
        let w_ = w.clone();

        wait_for_async_operation::<
          callback::AddScriptToExecuteOnDocumentCreatedCompletedHandler,
          callback::ErrorCodeArg,
          callback::StringArg,
        >(
          Box::new(move |handler| w_.AddScriptToExecuteOnDocumentCreated(js.as_str(), handler)),
          Box::new(|error_code, _id| error_code),
        )?;
      }
    }

    // Message handler
    unsafe {
      let mut _token = EventRegistrationToken::default();
      assert!(w
        .add_WebMessageReceived(
          callback::create::<callback::WebMessageReceivedEventHandler>(Box::new(
            move |webview: Option<webview2::ICoreWebView2>,
                  args: Option<webview2::ICoreWebView2WebMessageReceivedEventArgs>| {
              if let (Some(webview), Some(args)) = (webview, args) {
                let mut js = PWSTR::default();
                if args.TryGetWebMessageAsString(&mut js).is_ok() {
                  if let (js, Some(rpc_handler)) = (take_pwstr(js), rpc_handler.as_ref()) {
                    match super::rpc_proxy(js, rpc_handler) {
                      Ok(result) => {
                        if let Some(ref script) = result {
                          assert!(webview
                            .ExecuteScript(
                              script.as_str(),
                              callback::create::<callback::ExecuteScriptCompletedHandler>(
                                Box::new(|error_code, _result| error_code)
                              )
                              .unwrap(),
                            )
                            .is_ok());
                        }
                      }
                      Err(e) => {
                        eprintln!("{}", e);
                      }
                    }
                  }
                }
              }
              windows::ErrorCode::S_OK
            },
          ))?,
          &mut _token,
        )
        .is_ok());
    }

    let mut custom_protocol_name = None;
    if let Some((name, function)) = custom_protocol {
      // WebView2 doesn't support non-standard protocols yet, so we have to use this workaround
      // See https://github.com/MicrosoftEdge/WebView2Feedback/issues/73
      custom_protocol_name = Some(name.clone());

      unsafe {
        assert!(w
          .AddWebResourceRequestedFilter(
            format!("file://custom-protocol-{}*", name).as_str(),
            webview2::COREWEBVIEW2_WEB_RESOURCE_CONTEXT::COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
          )
          .is_ok());
        let env_ = env.clone();
        let mut token = EventRegistrationToken::default();
        assert!(w
          .add_WebResourceRequested(
            callback::create::<callback::WebResourceRequestedEventHandler>(Box::new(
              move |_webview: Option<webview2::ICoreWebView2>,
               args: Option<webview2::ICoreWebView2WebResourceRequestedEventArgs>| {
                if let Some(args) = args {
                  let mut request = None;
                  if args.get_Request(&mut request).is_ok() {
                    if let Some(request) = request {
                      let mut uri = PWSTR::default();
                      if request.get_Uri(&mut uri).is_ok() {
                        let uri = take_pwstr(uri);

                        // Undo the protocol workaround when giving path to resolver
                        let path = uri.replace(
                          &format!("file://custom-protocol-{}", name),
                          &format!("{}://", name),
                        );

                        if let Ok(content) = function(&path) {
                          let mime = MimeType::parse(&content, &uri);
                          let mut content =
                            Shell::SHCreateMemStream(content.as_ptr(), content.len() as u32);
                          if content.is_some() {
                            let mut response = None;
                            if env_
                              .CreateWebResourceResponse(
                                &mut content,
                                200,
                                "OK",
                                format!("Content-Type: {}", mime).as_str(),
                                &mut response,
                              )
                              .is_ok()
                            {
                              return args.put_Response(response);
                            }
                          }
                        }
                      }
                    }
                  }
                }
                windows::ErrorCode::S_OK
              },
            ))?,
            &mut token,
          )
          .is_ok());
      }
    }

    // Enable clipboard
    unsafe {
      let mut token = EventRegistrationToken::default();
      assert!(w.add_PermissionRequested(
        callback::create::<callback::PermissionRequestedEventHandler>(Box::new(
          move |_sender, args: Option<webview2::ICoreWebView2PermissionRequestedEventArgs>| {
            if let Some(args) = args {
              let mut permission_kind = webview2::COREWEBVIEW2_PERMISSION_KIND::COREWEBVIEW2_PERMISSION_KIND_UNKNOWN_PERMISSION;
              if args.get_PermissionKind(&mut permission_kind).is_ok() && permission_kind == webview2::COREWEBVIEW2_PERMISSION_KIND::COREWEBVIEW2_PERMISSION_KIND_CLIPBOARD_READ {
                return args.put_State(webview2::COREWEBVIEW2_PERMISSION_STATE::COREWEBVIEW2_PERMISSION_STATE_ALLOW);
              }
            }
            windows::ErrorCode::S_OK
          },
        ))?,
        &mut token,
      )
      .is_ok());
    }

    // Navigation
    if let Some(url) = url {
      if url.cannot_be_a_base() {
        let s = url.as_str();
        if let Some(pos) = s.find(',') {
          let (_, path) = s.split_at(pos + 1);
          unsafe {
            assert!(w.NavigateToString(path).is_ok());
          }
        }
      } else {
        let mut url_string = String::from(url.as_str());
        if let Some(name) = custom_protocol_name {
          if name == url.scheme() {
            // WebView2 doesn't support non-standard protocols yet, so we have to use this workaround
            // See https://github.com/MicrosoftEdge/WebView2Feedback/issues/73
            url_string = url.as_str().replace(
              &format!("{}://", name),
              &format!("file://custom-protocol-{}", name),
            )
          }
        }
        unsafe {
          assert!(w.Navigate(url_string).is_ok());
        }
      }
    }

    unsafe {
      assert!(controller.put_IsVisible(true).is_ok());
    }

    let _ = controller_rc.set(controller).expect("set the controller");
    let _ = webview_rc.set(w).expect("set the webview");

    if let Some(file_drop_handler) = file_drop_handler {
      let mut file_drop_controller = FileDropController::new();
      file_drop_controller.listen(hwnd, file_drop_handler);
      let _ = file_drop_controller_rc.set(file_drop_controller);
    }

    Ok(Self {
      controller: controller_rc,
      webview: webview_rc,
      file_drop_controller: file_drop_controller_rc,
    })
  }

  fn eval(&self, js: &str) -> Result<()> {
    if let Some(w) = self.webview.get() {
      unsafe {
        let error_code = w.ExecuteScript(
          js,
          callback::create::<callback::ExecuteScriptCompletedHandler>(Box::new(
            move |error_code, _result| error_code,
          ))?,
        );
        if error_code.is_err() {
          return Err(windows::Error::fast_error(error_code).into());
        }
      }
    }

    Ok(())
  }
}

impl InnerWebView {
  pub fn resize(&self, hwnd: HWND) -> Result<()> {
    // Safety: System calls are unsafe
    unsafe {
      let mut rect = RECT::default();
      WindowsAndMessaging::GetClientRect(hwnd, &mut rect);
      if let Some(c) = self.controller.get() {
        let (width, height) = (rect.right - rect.left, rect.bottom - rect.top);
        let error_code = c.put_Bounds(RECT {
          left: 0,
          top: 0,
          right: width,
          bottom: height,
        });
        if error_code.is_err() {
          return Err(windows::Error::fast_error(error_code).into());
        }
      }
    }

    Ok(())
  }
}

/// The WebView2 threading model runs everything on the UI thread, including callbacks which it triggers
/// with `PostMessage`, and we're using this here because it's waiting for some async operations in WebView2
/// to finish before starting the main message loop in `EventLoop::run`. As long as there are no pending
/// results in `rx`, it will poll the [`EventLoop`] with [`EventLoopExtRunReturn::run_return`] and check for a
/// result after each message is dispatched.
unsafe fn wait_for_async_operation<'a, T, Arg1, Arg2>(
  closure: Box<dyn FnOnce(<T as callback::Callback<'a>>::Interface) -> windows::ErrorCode>,
  completed: callback::CompletedClosure<'a, Arg1, Arg2>,
) -> Result<()>
where
  T: callback::Callback<'a, Closure = callback::CompletedClosure<'a, Arg1, Arg2>>,
  Arg1: callback::ClosureArg<'a>,
  Arg2: callback::ClosureArg<'a>,
{
  let (tx, rx) = mpsc::channel();
  let completed = Box::new(move |arg_1, arg_2| {
    tx.send(completed(arg_1, arg_2))
      .expect("send over mpsc channel");
    windows::ErrorCode::S_OK
  });
  let callback = callback::create::<'a, T>(completed)?;

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

pub fn string_from_pwstr(source: PWSTR) -> String {
  let mut buffer = Vec::new();
  let mut pwz = source.0;

  unsafe {
    while *pwz != 0 {
      buffer.push(*pwz);
      pwz = pwz.add(1);
    }
  }

  String::from_utf16(&buffer).expect("string_from_pwstr")
}

pub fn take_pwstr(source: PWSTR) -> String {
  let result = string_from_pwstr(source);

  if !source.0.is_null() {
    unsafe {
      Com::CoTaskMemFree(mem::transmute(source.0));
    }
  }

  result
}

pub fn pwstr_from_str(source: &str) -> PWSTR {
  if source.is_empty() {
    PWSTR::default()
  } else {
    let buffer: Vec<u16> = source.encode_utf16().collect();

    unsafe {
      let cch = source.len();
      let cb = (cch + 1) * size_of::<u16>();
      let result = PWSTR(mem::transmute(Com::CoTaskMemAlloc(cb)));
      let mut pwz = result.0;
      ptr::copy(buffer.as_ptr(), pwz, cch);
      pwz = pwz.add(cch);
      *pwz = 0u16;

      result
    }
  }
}
