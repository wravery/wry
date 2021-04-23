use super::pwstr::{pwstr_from_str, string_from_pwstr};

use windows::{Abi, Interface};

use bindings::{
  constants::TARGET_COMPATIBLE_BROWSER,
  Microsoft::Web::WebView2,
  Windows::Win32::SystemServices::{BOOL, E_NOINTERFACE, E_POINTER, PWSTR, S_OK},
};

/// Implementation of the [`WebView2::ICoreWebView2EnvironmentOptions`] COM interface.
#[repr(C)]
pub struct EnvironmentOptions {
  vtable: *const WebView2::ICoreWebView2EnvironmentOptions_abi,
  count: windows::RefCount,
  /// Storage for [`WebView2::ICoreWebView2EnvironmentOptions::get_AdditionalBrowserArguments`] and
  /// [`WebView2::ICoreWebView2EnvironmentOptions::put_AdditionalBrowserArguments`].
  additional_browser_arguments: String,
  /// Storage for [`WebView2::ICoreWebView2EnvironmentOptions::get_Language`] and
  /// [`WebView2::ICoreWebView2EnvironmentOptions::put_Language`].
  language: String,
  /// Storage for [`WebView2::ICoreWebView2EnvironmentOptions::get_TargetCompatibleBrowserVersion`] and
  /// [`WebView2::ICoreWebView2EnvironmentOptions::put_TargetCompatibleBrowserVersion`].
  target_compatible_browser: String,
  /// Storage for [`WebView2::ICoreWebView2EnvironmentOptions::get_AllowSingleSignOnUsingOSPrimaryAccount`]
  /// and [`WebView2::ICoreWebView2EnvironmentOptions::put_AllowSingleSignOnUsingOSPrimaryAccount`].
  allow_single_sign_on_using_os_primary_account: bool,
}

#[allow(non_snake_case)]
impl EnvironmentOptions {
  /// Factory method which returns a [`windows::Result<WebView2::ICoreWebView2EnvironmentOptions>`] wrapped
  /// around a new instance of [`EnvironmentOptions`].
  pub fn create() -> windows::Result<WebView2::ICoreWebView2EnvironmentOptions> {
    let options = Box::new(Self::new());
    let options = unsafe { Self::from_abi(Box::into_raw(options) as windows::RawPtr)? };
    Ok(options)
  }

  unsafe fn from_abi(
    this: windows::RawPtr,
  ) -> windows::Result<WebView2::ICoreWebView2EnvironmentOptions> {
    let unknown = windows::IUnknown::from_abi(this)?;
    unknown.vtable().1(unknown.abi()); // add_ref to balance the release called in IUnknown::drop
    unknown.cast()
  }

  fn new() -> Self {
    static VTABLE: WebView2::ICoreWebView2EnvironmentOptions_abi =
      WebView2::ICoreWebView2EnvironmentOptions_abi(
        EnvironmentOptions::QueryInterface,
        EnvironmentOptions::AddRef,
        EnvironmentOptions::Release,
        EnvironmentOptions::get_AdditionalBrowserArguments,
        EnvironmentOptions::put_AdditionalBrowserArguments,
        EnvironmentOptions::get_Language,
        EnvironmentOptions::put_Language,
        EnvironmentOptions::get_TargetCompatibleBrowserVersion,
        EnvironmentOptions::put_TargetCompatibleBrowserVersion,
        EnvironmentOptions::get_AllowSingleSignOnUsingOSPrimaryAccount,
        EnvironmentOptions::put_AllowSingleSignOnUsingOSPrimaryAccount,
      );

    Self {
      vtable: &VTABLE,
      count: windows::RefCount::new(),
      additional_browser_arguments: String::new(),
      language: String::new(),
      target_compatible_browser: String::from(TARGET_COMPATIBLE_BROWSER),
      allow_single_sign_on_using_os_primary_account: false,
    }
  }

  unsafe extern "system" fn QueryInterface(
    this: windows::RawPtr,
    iid: &windows::Guid,
    interface: *mut windows::RawPtr,
  ) -> windows::HRESULT {
    if interface.is_null() {
      E_POINTER
    } else if *iid == windows::IUnknown::IID
      || *iid == WebView2::ICoreWebView2EnvironmentOptions::IID
    {
      Self::AddRef(this);
      *interface = this;
      S_OK
    } else {
      E_NOINTERFACE
    }
  }

  unsafe extern "system" fn AddRef(this: windows::RawPtr) -> u32 {
    let interface = this as *mut Self;
    (*interface).count.add_ref()
  }

  unsafe extern "system" fn Release(this: windows::RawPtr) -> u32 {
    let interface = this as *mut Self;
    let count = (*interface).count.release();
    if count == 0 {
      // Destroy the underlying data
      Box::from_raw(interface);
    }
    count
  }

  unsafe extern "system" fn get_AdditionalBrowserArguments(
    this: windows::RawPtr,
    value: *mut PWSTR,
  ) -> windows::HRESULT {
    let interface = this as *const Self;
    *value = pwstr_from_str(&(*interface).additional_browser_arguments);
    S_OK
  }

  unsafe extern "system" fn put_AdditionalBrowserArguments(
    this: windows::RawPtr,
    value: PWSTR,
  ) -> windows::HRESULT {
    let interface = this as *mut Self;
    (*interface).additional_browser_arguments = string_from_pwstr(value);
    S_OK
  }

  unsafe extern "system" fn get_Language(
    this: windows::RawPtr,
    value: *mut PWSTR,
  ) -> windows::HRESULT {
    let interface = this as *const Self;
    *value = pwstr_from_str(&(*interface).language);
    S_OK
  }

  unsafe extern "system" fn put_Language(this: windows::RawPtr, value: PWSTR) -> windows::HRESULT {
    let interface = this as *mut Self;
    (*interface).language = string_from_pwstr(value);
    S_OK
  }

  unsafe extern "system" fn get_TargetCompatibleBrowserVersion(
    this: windows::RawPtr,
    value: *mut PWSTR,
  ) -> windows::HRESULT {
    let interface = this as *const Self;
    *value = pwstr_from_str(&(*interface).target_compatible_browser);
    S_OK
  }

  unsafe extern "system" fn put_TargetCompatibleBrowserVersion(
    this: windows::RawPtr,
    value: PWSTR,
  ) -> windows::HRESULT {
    let interface = this as *mut Self;
    (*interface).target_compatible_browser = string_from_pwstr(value);
    S_OK
  }

  unsafe extern "system" fn get_AllowSingleSignOnUsingOSPrimaryAccount(
    this: windows::RawPtr,
    allow: *mut BOOL,
  ) -> windows::HRESULT {
    let interface = this as *const Self;
    *allow = BOOL::from((*interface).allow_single_sign_on_using_os_primary_account);
    S_OK
  }

  unsafe extern "system" fn put_AllowSingleSignOnUsingOSPrimaryAccount(
    this: windows::RawPtr,
    allow: BOOL,
  ) -> windows::HRESULT {
    let interface = this as *mut Self;
    (*interface).allow_single_sign_on_using_os_primary_account = allow.as_bool();
    S_OK
  }
}
