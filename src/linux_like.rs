use crate::{NamedPipe, Pipe, last_err};
use napi::bindgen_prelude::*;
use napi_derive::napi;
use std::ffi::CString;

fn memfd_create(name: &str, flags: u32) -> Result<i32> {
  let cname =
    CString::new(name).map_err(|_| Error::from_reason("memfd name cannot contain NUL bytes"))?;

  // SAFETY: syscall boundary; cname is a valid C string.
  let fd = unsafe { libc::memfd_create(cname.as_ptr(), flags) };

  if fd < 0 {
    return Err(last_err("memfd_create failed"));
  }

  Ok(fd as i32)
}

#[napi(js_name = "memfd_create")]
pub fn memfd_create_js(name: String, flags: u32) -> Result<i32> {
  memfd_create(&name, flags)
}

pub fn share_memfd(payload: Buffer, name: Option<String>) -> Result<Pipe> {
  let name = name.unwrap_or_else(|| "share".to_string());
  let fd = memfd_create(name.as_str(), 0)?;
  unsafe {
    if libc::ftruncate(fd, 0) < 0 {
      return Err(last_err("ftruncate failed"));
    }
  };

  let bytes = payload.to_vec();

  unsafe {
    if libc::write(fd, bytes.as_ptr() as *const libc::c_void, bytes.len()) < 0 {
      return Err(last_err("write failed"));
    }
  };

  // unsafe {
  //   if libc::lseek(fd, 0, libc::SEEK_SET) < 0 {
  //     return Err(last_err("lseek failed"));
  //   }
  // };

  Ok(Pipe { fd, name })
}

#[napi(js_name = "shareMemFD")]
pub fn share_memfd_js(payload: Buffer, name: Option<String>) -> Result<Pipe> {
  share_memfd(payload, name)
}

#[napi(js_name = "set_cloexec")]
pub fn set_cloexec(fd: i32, enabled: bool) -> Result<()> {
  let old = unsafe { libc::fcntl(fd, libc::F_GETFD) };
  if old < 0 {
    return Err(last_err("F_GETFD failed"));
  }

  let new_flags = if enabled {
    old | libc::FD_CLOEXEC
  } else {
    old & !libc::FD_CLOEXEC
  };

  let rc = unsafe { libc::fcntl(fd, libc::F_SETFD, new_flags) };
  if rc < 0 {
    return Err(last_err("F_SETFD failed"));
  }

  Ok(())
}

#[napi(js_name = "share")]
pub fn share_js(payload: Buffer, name: Option<String>) -> Result<Either<Pipe, NamedPipe>> {
  Ok(Either::A(share_memfd(payload, name)?))
}
