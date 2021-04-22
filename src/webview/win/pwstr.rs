use std::{
  mem::{self, size_of},
  ptr,
};

use bindings::Windows::Win32::{Com, SystemServices::PWSTR};

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
