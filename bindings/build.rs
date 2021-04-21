#[macro_use]
extern crate thiserror;

fn main() -> webview2_nuget::Result<()> {
  webview2_nuget::install()?;
  webview2_nuget::update_lib_path()?;
  webview2_nuget::update_constants()?;

  windows::build!(
      Microsoft::Web::WebView2::*,
      Windows::Storage::Streams::*,
      Windows::Win32::Com::*,
      Windows::Win32::DisplayDevices::{
        POINT,
        POINTL,
        RECT,
        SIZE
      },
      Windows::Win32::Gdi::UpdateWindow,
      Windows::Win32::HiDpi::{
        PROCESS_DPI_AWARENESS,
        SetProcessDpiAwareness
      },
      Windows::Win32::KeyboardAndMouseInput::SetFocus,
      Windows::Win32::MenusAndResources::HMENU,
      Windows::Win32::Shell::{
        DragFinish,
        DragQueryFileW,
        HDROP,
        ITaskbarList,
        SHCreateMemStream,
        TaskbarList
      },
      Windows::Win32::SystemServices::{
        BOOL,
        CLIPBOARD_FORMATS,
        DRAGDROP_E_INVALIDHWND,
        DV_E_FORMATETC,
        E_NOINTERFACE,
        E_POINTER,
        GetCurrentThreadId,
        GetModuleHandleA,
        HINSTANCE,
        LRESULT,
        PWSTR,
        S_OK,
        userHMETAFILEPICT,
        userHENHMETAFILE
      },
      Windows::Win32::WindowsAndMessaging::*,
      Windows::Win32::WinRT::EventRegistrationToken,
  );

  println!("cargo:rerun-if-changed=build.rs");

  Ok(())
}

mod webview2_nuget {
  use io::Write;
  use regex::Regex;
  use std::{
    convert::From,
    env, fs,
    io::{self, BufRead},
    path::PathBuf,
    process::Command,
  };

  const WEBVIEW2_NAME: &str = "Microsoft.Web.WebView2";
  const WEBVIEW2_VERSION: &str = "1.0.774.44";

  pub fn install() -> Result<()> {
    let manifest_dir = get_manifest_dir()?;
    let install_root = match manifest_dir.to_str() {
      Some(path) => path,
      None => return Err(Error::MissingPath(manifest_dir)),
    };

    let package_root = get_package_root_dir(manifest_dir.clone())?;

    if !check_nuget_dir(install_root)? {
      let mut nuget_path = manifest_dir.clone();
      nuget_path.push("tools");
      nuget_path.push("nuget.exe");

      let nuget_tool = match nuget_path.to_str() {
        Some(path) => path,
        None => return Err(Error::MissingPath(nuget_path)),
      };

      Command::new(nuget_tool)
        .args(&[
          "install",
          WEBVIEW2_NAME,
          "-OutputDirectory",
          install_root,
          "-Version",
          WEBVIEW2_VERSION,
        ])
        .output()?;

      if !check_nuget_dir(install_root)? {
        return Err(Error::MissingPath(package_root));
      }

      update_windows(package_root)?;
    }

    Ok(())
  }

  fn get_manifest_dir() -> Result<PathBuf> {
    Ok(PathBuf::from(env::var("CARGO_MANIFEST_DIR")?))
  }

  fn get_nuget_path() -> String {
    format!("{}.{}", WEBVIEW2_NAME, WEBVIEW2_VERSION)
  }

  fn get_package_root_dir(manifest_dir: PathBuf) -> Result<PathBuf> {
    let mut package_root = manifest_dir;
    package_root.push(get_nuget_path());
    Ok(package_root)
  }

  fn check_nuget_dir(install_root: &str) -> Result<bool> {
    let nuget_path = get_nuget_path();
    let mut dir_iter = fs::read_dir(install_root)?.filter(|dir| match dir {
      Ok(dir) => match dir.file_type() {
        Ok(file_type) => {
          file_type.is_dir()
            && match dir.file_name().to_str() {
              Some(name) => name.eq_ignore_ascii_case(&nuget_path),
              None => false,
            }
        }
        Err(_) => false,
      },
      Err(_) => false,
    });
    Ok(dir_iter.next().is_some())
  }

  fn update_windows(package_root: PathBuf) -> Result<()> {
    let mut windows_dir = get_workspace_dir()?;
    windows_dir.push(".windows");
    fs::create_dir_all(windows_dir.as_path())?;

    const WEBVIEW2_LICENSE: &str = "WebView2Loader.dll.LICENSE.txt";
    const LICENSE_TXT: &str = "LICENSE.txt";

    let mut license_dest = windows_dir.clone();
    license_dest.push(WEBVIEW2_LICENSE);
    let mut license_src = package_root.clone();
    license_src.push(LICENSE_TXT);
    fs::copy(license_src.as_path(), license_dest.as_path())?;

    const WEBVIEW2_DLL: &str = "WebView2Loader.dll";
    const WEBVIEW2_LIB: &str = "WebView2Loader.dll.lib";
    const WEBVIEW2_TARGETS: &[&'static str] = &["arm64", "x64", "x86"];

    let mut native_dir = package_root;
    native_dir.push("build");
    native_dir.push("native");
    for &target in WEBVIEW2_TARGETS {
      let mut dll_dest = windows_dir.clone();
      dll_dest.push(target);
      fs::create_dir_all(dll_dest.as_path())?;
      let mut lib_dest = dll_dest.clone();
      let mut dll_src = native_dir.clone();
      dll_src.push(target);
      let mut lib_src = dll_src.clone();
      dll_dest.push(WEBVIEW2_DLL);
      dll_src.push(WEBVIEW2_DLL);
      eprintln!("Copy from {:?} -> {:?}", dll_src, dll_dest);
      fs::copy(dll_src.as_path(), dll_dest.as_path())?;
      lib_dest.push(WEBVIEW2_LIB);
      lib_src.push(WEBVIEW2_LIB);
      eprintln!("Copy from {:?} -> {:?}", lib_src, lib_dest);
      fs::copy(lib_src.as_path(), lib_dest.as_path())?;
    }

    Ok(())
  }

  fn get_arch() -> Result<String> {
    let target = env::var("TARGET")?;
    let arch = if target.contains("x86_64") {
      "x64"
    } else if target.contains("aarch64") {
      "arm64"
    } else {
      "x86"
    };
    Ok(String::from(arch))
  }

  pub fn update_lib_path() -> Result<()> {
    let mut lib_path = get_package_root_dir(get_manifest_dir()?)?;
    lib_path.push("build");
    lib_path.push("native");
    lib_path.push(get_arch()?);
    let lib_path = match lib_path.to_str() {
      Some(path) => path,
      None => return Err(Error::MissingPath(lib_path)),
    };

    println!("cargo:rustc-link-search=native={}", lib_path);
    Ok(())
  }

  include!("src/constants.rs");

  pub fn update_constants() -> Result<()> {
    let mut header_path = get_package_root_dir(get_manifest_dir()?)?;
    header_path.push("build");
    header_path.push("native");
    header_path.push("include");
    header_path.push("WebView2EnvironmentOptions.h");

    let header = fs::File::open(match header_path.to_str() {
      Some(path) => Ok(path),
      None => Err(Error::MissingPath(header_path.clone())),
    }?)?;
    let reader = io::BufReader::new(header);
    let re = Regex::new(r#"^\s*#define\s+CORE_WEBVIEW_TARGET_PRODUCT_VERSION\s+L"([^"]+)"\s*$"#)?;
    let target_compatible_browser = reader
      .lines()
      .filter_map(|line| match line {
        Ok(line) => match re.captures(line.as_str()) {
          Some(cap) => cap.get(1).map(|m| String::from(m.as_str())),
          None => None,
        },
        _ => None,
      })
      .next();

    match target_compatible_browser {
      Some(target_compatible_browser) => {
        if target_compatible_browser != TARGET_COMPATIBLE_BROWSER {
          let mut constants_path = PathBuf::new();
          constants_path.push("src");
          constants_path.push("constants.rs");
          let constants = fs::File::create(match constants_path.to_str() {
            Some(path) => Ok(path),
            None => Err(Error::MissingPath(constants_path)),
          }?)?;
          let mut writer = io::BufWriter::new(constants);
          writer.write_all(
            format!(
              "pub const TARGET_COMPATIBLE_BROWSER: &str = \"{}\";\n",
              target_compatible_browser
            )
            .as_bytes(),
          )?;
        }

        Ok(())
      }
      None => Err(Error::MissingPath(header_path.clone())),
    }
  }

  fn get_workspace_dir() -> Result<PathBuf> {
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct CargoMetadata {
      workspace_root: String,
    }

    let output = Command::new(env::var("CARGO")?)
      .args(&["metadata", "--format-version=1", "--no-deps", "--offline"])
      .output()?;

    let metadata: CargoMetadata = serde_json::from_slice(&output.stdout)?;

    Ok(PathBuf::from(metadata.workspace_root))
  }

  #[derive(Debug, Error)]
  pub enum Error {
    #[error(transparent)]
    IoError(#[from] io::Error),
    #[error(transparent)]
    VarError(#[from] env::VarError),
    #[error(transparent)]
    JsonError(#[from] serde_json::Error),
    #[error(transparent)]
    RegexError(#[from] regex::Error),
    #[error("Missing Path")]
    MissingPath(PathBuf),
  }

  pub type Result<T> = std::result::Result<T, Error>;
}
