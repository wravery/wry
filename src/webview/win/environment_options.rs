use super::{pwstr_from_str, string_from_pwstr};

use std::{
  mem,
  sync::atomic::{AtomicU32, Ordering},
};

use windows::{Abi, Interface};

use bindings::{
  constants::TARGET_COMPATIBLE_BROWSER,
  Microsoft::Web::WebView2,
  Windows::Win32::SystemServices::{BOOL, E_NOINTERFACE, E_POINTER, PWSTR, S_OK},
};

unsafe fn from_abi<I: Interface>(this: windows::RawPtr) -> windows::Result<I> {
  let unknown = windows::IUnknown::from_abi(this)?;
  unknown.vtable().1(unknown.abi()); // add_ref to balance the release called in IUnknown::drop
  Ok(unknown.cast()?)
}

pub unsafe fn create_options() -> windows::Result<WebView2::ICoreWebView2EnvironmentOptions> {
  let options = Box::new(EnvironmentOptions::new());
  let options = from_abi(Box::into_raw(options) as windows::RawPtr)?;
  Ok(options)
}

#[repr(C)]
pub struct EnvironmentOptions {
  vtable: *const WebView2::ICoreWebView2EnvironmentOptions_abi,
  refcount: AtomicU32,
  additional_browser_arguments: String,
  language: String,
  target_compatible_browser: String,
  allow_single_sign_on_using_os_primary_account: bool,
}

impl EnvironmentOptions {
  fn new() -> Self {
    static VTABLE: WebView2::ICoreWebView2EnvironmentOptions_abi =
      WebView2::ICoreWebView2EnvironmentOptions_abi(
        EnvironmentOptions::query_interface,
        EnvironmentOptions::add_ref,
        EnvironmentOptions::release,
        EnvironmentOptions::get_additional_browser_arguments,
        EnvironmentOptions::put_additional_browser_arguments,
        EnvironmentOptions::get_language,
        EnvironmentOptions::put_language,
        EnvironmentOptions::get_target_compatible_browser,
        EnvironmentOptions::put_target_compatible_browser,
        EnvironmentOptions::get_allow_single_sign_on_using_os_primary_account,
        EnvironmentOptions::put_allow_single_sign_on_using_os_primary_account,
      );

    Self {
      vtable: &VTABLE,
      refcount: AtomicU32::new(1),
      additional_browser_arguments: String::new(),
      language: String::new(),
      target_compatible_browser: String::from(TARGET_COMPATIBLE_BROWSER),
      allow_single_sign_on_using_os_primary_account: false,
    }
  }

  unsafe extern "system" fn query_interface(
    this: windows::RawPtr,
    iid: &windows::Guid,
    interface: *mut windows::RawPtr,
  ) -> windows::HRESULT {
    if interface.is_null() {
      E_POINTER
    } else if *iid == windows::IUnknown::IID
      || *iid == WebView2::ICoreWebView2EnvironmentOptions::IID
    {
      Self::add_ref(this);
      *interface = this;
      S_OK
    } else {
      E_NOINTERFACE
    }
  }

  unsafe extern "system" fn add_ref(this: windows::RawPtr) -> u32 {
    let interface: *mut Self = mem::transmute(this);
    let count = (*interface).refcount.fetch_add(1, Ordering::Release) + 1;
    count
  }

  unsafe extern "system" fn release(this: windows::RawPtr) -> u32 {
    let interface: *mut Self = mem::transmute(this);
    let count = (*interface).refcount.fetch_sub(1, Ordering::Release) - 1;
    if count == 0 {
      // Destroy the underlying data
      Box::from_raw(interface);
    }
    count
  }

  unsafe extern "system" fn get_additional_browser_arguments(
    this: windows::RawPtr,
    value: *mut PWSTR,
  ) -> windows::HRESULT {
    let interface: *const Self = mem::transmute(this);
    *value = pwstr_from_str(&(*interface).additional_browser_arguments);
    S_OK
  }

  unsafe extern "system" fn put_additional_browser_arguments(
    this: windows::RawPtr,
    value: PWSTR,
  ) -> windows::HRESULT {
    let interface: *mut Self = mem::transmute(this);
    (*interface).additional_browser_arguments = string_from_pwstr(value);
    S_OK
  }

  unsafe extern "system" fn get_language(
    this: windows::RawPtr,
    value: *mut PWSTR,
  ) -> windows::HRESULT {
    let interface: *const Self = mem::transmute(this);
    *value = pwstr_from_str(&(*interface).language);
    S_OK
  }

  unsafe extern "system" fn put_language(
    this: windows::RawPtr,
    value: PWSTR,
  ) -> windows::HRESULT {
    let interface: *mut Self = mem::transmute(this);
    (*interface).language = string_from_pwstr(value);
    S_OK
  }

  unsafe extern "system" fn get_target_compatible_browser(
    this: windows::RawPtr,
    value: *mut PWSTR,
  ) -> windows::HRESULT {
    let interface: *const Self = mem::transmute(this);
    *value = pwstr_from_str(&(*interface).target_compatible_browser);
    S_OK
  }

  unsafe extern "system" fn put_target_compatible_browser(
    this: windows::RawPtr,
    value: PWSTR,
  ) -> windows::HRESULT {
    let interface: *mut Self = mem::transmute(this);
    (*interface).target_compatible_browser = string_from_pwstr(value);
    S_OK
  }

  unsafe extern "system" fn get_allow_single_sign_on_using_os_primary_account(
    this: windows::RawPtr,
    allow: *mut BOOL,
  ) -> windows::HRESULT {
    let interface: *const Self = mem::transmute(this);
    *allow = BOOL::from((*interface).allow_single_sign_on_using_os_primary_account);
    S_OK
  }

  unsafe extern "system" fn put_allow_single_sign_on_using_os_primary_account(
    this: windows::RawPtr,
    allow: BOOL,
  ) -> windows::HRESULT {
    let interface: *mut Self = mem::transmute(this);
    (*interface).allow_single_sign_on_using_os_primary_account = allow.as_bool();
    S_OK
  }
}
