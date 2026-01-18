#![deny(clippy::all)]

#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "netbsd"))]
pub mod linux_like;
#[cfg(target_family = "unix")]
pub mod unix;
#[cfg(target_family = "windows")]
pub mod windows;

use napi::bindgen_prelude::*;
use napi_derive::napi;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

fn last_err(msg: &str) -> Error {
  let e = std::io::Error::last_os_error();
  Error::from_reason(format!("{msg}: {e}"))
}

#[napi]
pub struct Pipe {
  fd: i32,
  name: String, // Not entirely relevant
}

#[napi]
pub struct NamedPipe {
  path: String,
  should_stop: Arc<AtomicBool>,
  is_closed: Arc<AtomicBool>,
  #[cfg(windows)]
  handle: Arc<std::os::windows::io::OwnedHandle>,
}

#[cfg(test)]
mod tests {
  use ulid::Ulid;

  #[test]
  fn it_works() {
    let result = format!("/share-{}-{}", std::process::id(), Ulid::new().to_string());
    dbg!("{:?}", result); //share-186352-01KF0MCZ1S4H80VP3A8JS5NA02
    assert_eq!(1, 1);
  }
}
