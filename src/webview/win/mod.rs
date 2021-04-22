mod callback;
mod environment_options;

mod file_drop;

use bindings::{
  Microsoft::Web::WebView2 as webview2,
  Windows::Win32::{
    Com,
    DisplayDevices::RECT,
    Shell,
    SystemServices::{E_NOINTERFACE, PWSTR, S_OK},
    WinRT::EventRegistrationToken,
    WindowsAndMessaging::{self, HWND},
  },
};

use crate::{
  webview::{mimetype::MimeType, WV},
  FileDropHandler, Result, RpcHandler,
};

use file_drop::FileDropController;

use std::{
  mem::{self, size_of},
  path::PathBuf,
  ptr,
  rc::Rc,
};

use once_cell::unsync::OnceCell;
use url::Url;
use winit::{platform::windows::WindowExtWindows, window::Window};

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

    let env = {
      let mut result = Err(windows::Error::fast_error(E_NOINTERFACE));

      callback::CreateCoreWebView2EnvironmentCompletedHandler::wait_for_async_operation(
        Box::new(|environmentcreatedhandler| match user_data_path {
          Some(user_data_path_provided) => unsafe {
            webview2::CreateCoreWebView2EnvironmentWithOptions(
              "",
              user_data_path_provided.to_str().unwrap_or(""),
              environment_options::create_options().map_or(None, |options| Some(options)),
              environmentcreatedhandler,
            )
          },
          None => unsafe { webview2::CreateCoreWebView2Environment(environmentcreatedhandler) },
        }),
        Box::new(|error_code, environment| {
          result = error_code.and_some(environment);
          error_code
        }),
      )?;

      result
    }?;

    // Webview controller
    let controller = {
      let mut result = Err(windows::Error::fast_error(E_NOINTERFACE));
      let env_ = env.clone();

      callback::CreateCoreWebView2ControllerCompletedHandler::wait_for_async_operation(
        Box::new(move |handler| unsafe { env_.CreateCoreWebView2Controller(hwnd, handler) }),
        Box::new(|error_code, controller| {
          result = error_code.and_some(controller);
          error_code
        }),
      )?;

      result
    }?;

    let w = unsafe {
      let mut result = None;
      controller.get_CoreWebView2(&mut result).and_some(result)?
    };

    // Enable sensible defaults
    unsafe {
      let mut result = None;
      let settings = w.get_Settings(&mut result).and_some(result)?;

      settings.put_IsStatusBarEnabled(false).ok()?;
      settings.put_AreDefaultContextMenusEnabled(true).ok()?;
      settings.put_IsZoomControlEnabled(false).ok()?;
      settings.put_AreDevToolsEnabled(false).ok()?;
      debug_assert!(settings.put_AreDevToolsEnabled(true).is_ok());
    }

    // Safety: System calls are unsafe
    unsafe {
      let mut rect = RECT::default();
      WindowsAndMessaging::GetClientRect(hwnd, &mut rect);
      let (width, height) = (rect.right - rect.left, rect.bottom - rect.top);
      controller
        .put_Bounds(RECT {
          left: 0,
          top: 0,
          right: width,
          bottom: height,
        })
        .ok()?;
    }

    // Initialize scripts
    let w_ = w.clone();

    callback::AddScriptToExecuteOnDocumentCreatedCompletedHandler::wait_for_async_operation(
      Box::new(move |handler| unsafe {
        w_.AddScriptToExecuteOnDocumentCreated(
          "window.external={invoke:s=>window.chrome.webview.postMessage(s)}",
          handler,
        )
      }),
      Box::new(|error_code, _id| error_code),
    )?;

    for js in scripts {
      let w_ = w.clone();

      callback::AddScriptToExecuteOnDocumentCreatedCompletedHandler::wait_for_async_operation(
        Box::new(move |handler| unsafe {
          w_.AddScriptToExecuteOnDocumentCreated(js.as_str(), handler)
        }),
        Box::new(|error_code, _id| error_code),
      )?;
    }

    // Message handler
    unsafe {
      let mut _token = EventRegistrationToken::default();
      w.add_WebMessageReceived(
        callback::WebMessageReceivedEventHandler::create(Box::new(move |webview, args| {
          if let (Some(webview), Some(args)) = (webview, args) {
            let mut js = PWSTR::default();
            if args.TryGetWebMessageAsString(&mut js).is_ok() {
              if let (js, Some(rpc_handler)) = (take_pwstr(js), rpc_handler.as_ref()) {
                match super::rpc_proxy(js, rpc_handler) {
                  Ok(result) => {
                    if let Some(ref script) = result {
                      match webview.ExecuteScript(script.as_str(), None).ok() {
                        Ok(_) => (),
                        Err(e) => {
                          eprintln!("{}", e);
                        }
                      };
                    }
                  }
                  Err(e) => {
                    eprintln!("{}", e);
                  }
                }
              }
            }
          }
          S_OK
        }))?,
        &mut _token,
      )
      .ok()?;
    }

    let mut custom_protocol_name = None;
    if let Some((name, function)) = custom_protocol {
      // WebView2 doesn't support non-standard protocols yet, so we have to use this workaround
      // See https://github.com/MicrosoftEdge/WebView2Feedback/issues/73
      custom_protocol_name = Some(name.clone());

      unsafe {
        w.AddWebResourceRequestedFilter(
          format!("file://custom-protocol-{}*", name).as_str(),
          webview2::COREWEBVIEW2_WEB_RESOURCE_CONTEXT::COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
        )
        .ok()?;
        let env_ = env;
        let mut token = EventRegistrationToken::default();
        w.add_WebResourceRequested(
          callback::WebResourceRequestedEventHandler::create(Box::new(move |_webview, args| {
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
            S_OK
          }))?,
          &mut token,
        )
        .ok()?;
      }
    }

    // Enable clipboard
    unsafe {
      let mut token = EventRegistrationToken::default();
      w.add_PermissionRequested(
        callback::PermissionRequestedEventHandler::create(Box::new(
          move |_sender, args| {
            if let Some(args) = args {
              let mut permission_kind = webview2::COREWEBVIEW2_PERMISSION_KIND::COREWEBVIEW2_PERMISSION_KIND_UNKNOWN_PERMISSION;
              if args.get_PermissionKind(&mut permission_kind).is_ok() && permission_kind == webview2::COREWEBVIEW2_PERMISSION_KIND::COREWEBVIEW2_PERMISSION_KIND_CLIPBOARD_READ {
                return args.put_State(webview2::COREWEBVIEW2_PERMISSION_STATE::COREWEBVIEW2_PERMISSION_STATE_ALLOW);
              }
            }
            S_OK
          },
        ))?,
        &mut token,
      )
      .ok()?;
    }

    // Navigation
    if let Some(url) = url {
      if url.cannot_be_a_base() {
        let s = url.as_str();
        if let Some(pos) = s.find(',') {
          let (_, path) = s.split_at(pos + 1);
          unsafe {
            w.NavigateToString(path).ok()?;
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
          w.Navigate(url_string).ok()?;
        }
      }
    }

    unsafe {
      controller.put_IsVisible(true).ok()?;
    }

    let _ = controller_rc.set(controller).expect("set the controller");
    let _ = webview_rc.set(w).expect("set the webview");

    if let Some(file_drop_handler) = file_drop_handler {
      let mut file_drop_controller = FileDropController::new();
      file_drop_controller.listen(hwnd, file_drop_handler);
      file_drop_controller_rc
        .set(file_drop_controller)
        .unwrap_or_default();
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
        w.ExecuteScript(js, None).ok()?;
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
        c.put_Bounds(RECT {
          left: 0,
          top: 0,
          right: width,
          bottom: height,
        })
        .ok()?;
      }
    }

    Ok(())
  }
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
