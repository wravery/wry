use std::{
  mem::{self, size_of},
  ptr,
};

use bindings::Windows::Win32::{Com, SystemServices::PWSTR};

/// Copy a [`PWSTR`] from an input param to a [`String`].
pub fn string_from_pwstr(source: PWSTR) -> String {
  if source.is_null() {
    String::new()
  } else {
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
}

/// Copy a [`PWSTR`] allocated with [`Com::CoTaskMemAlloc`] from an input param to a [`String`]
/// and free the original buffer with [`Com::CoTaskMemFree`].
pub fn take_pwstr(source: PWSTR) -> String {
  let result = string_from_pwstr(source);

  if !source.is_null() {
    unsafe {
      Com::CoTaskMemFree(mem::transmute(source.0));
    }
  }

  result
}

/// Copy a [`str`] slice to a [`PWSTR`] allocated with [`Com::CoTaskMemAlloc`].
pub fn pwstr_from_str(source: &str) -> PWSTR {
  if source.is_empty() {
    PWSTR::default()
  } else {
    let buffer: Vec<u16> = source.encode_utf16().chain(std::iter::once(0)).collect();

    unsafe {
      let cb = buffer.len() * size_of::<u16>();
      let result = PWSTR(mem::transmute(Com::CoTaskMemAlloc(cb)));
      ptr::copy(buffer.as_ptr(), result.0, buffer.len());

      result
    }
  }
}
