#![deny(clippy::all)]

use napi::bindgen_prelude::*;
use napi_derive::napi;

fn last_err(msg: &str) -> Error {
  let e = std::io::Error::last_os_error();
  Error::from_reason(format!("{msg}: {e}"))
}

#[cfg(target_family = "unix")]
mod unix {
  use super::*;
  use napi::Error;
  use std::cmp;
  use std::ffi::CString;
  use ulid::Ulid;

  #[napi]
  pub struct Pipe {
    fd: i32,
    name: String, // Not entirely relevant
  }

  #[napi]
  impl Pipe {
    #[napi(getter)]
    pub fn fd(&self) -> i32 {
      self.fd
    }

    #[napi(getter)]
    pub fn name(&self) -> String {
      self.name.clone()
    }

    #[napi]
    pub fn close(&self) -> Result<()> {
      let closed = unsafe { libc::close(self.fd) };

      if closed < 0 {
        return Err(last_err("close failed"));
      }
      Ok(())
    }
  }

  #[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "netbsd"))]
  mod linux_like {
    use super::*;

    fn memfd_create(name: &str, flags: u32) -> Result<i32> {
      let cname = CString::new(name)
        .map_err(|_| Error::from_reason("memfd name cannot contain NUL bytes"))?;

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

    #[napi(js_name = "share")]
    pub fn share_js(payload: Buffer, name: Option<String>) -> Result<Pipe> {
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
  }

  fn shm_open(name: &str, flags: i32, mode: u32) -> Result<i32> {
    // shm_open name must begin with "/" and usually must not contain other '/'
    // Make something unique-ish (pid + name), consider using pointer of name as well
    #[cfg(target_os = "macos")]
    let name = format!("/shr-{}", &name[..cmp::min(name.len(), 26)]);
    #[cfg(not(target_os = "macos"))]
    let name = format!(
      "/shr-{}",
      &name[..cmp::min(name.len(), libc::NAME_MAX as usize)]
    );
    let cname = CString::new(name).map_err(|_| Error::from_reason("name contains NUL"))?;

    let fd = unsafe { libc::shm_open(cname.as_ptr(), flags, mode) };
    if fd < 0 {
      return Err(last_err("shm_open failed"));
    }

    // Remove the name immediately; the object lives until all fds are closed
    unsafe {
      libc::shm_unlink(cname.as_ptr());
    }

    Ok(fd as i32)
  }

  #[napi(js_name = "shm_open")]
  pub fn shm_open_js(name: String, flags: i32, mode: u32) -> Result<i32> {
    shm_open(&name, flags, mode)
  }

  fn share_shm(payload: Buffer, name: Option<String>) -> Result<Pipe> {
    let name = name.unwrap_or_else(|| Ulid::new().to_string());
    let fd = shm_open(
      name.as_str(),
      libc::O_CREAT | libc::O_EXCL | libc::O_RDWR,
      0o600,
    )?;
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

  #[napi(js_name = "shareSHM")]
  pub fn share_shm_js(payload: Buffer, name: Option<String>) -> Result<Pipe> {
    share_shm(payload, name)
  }

  #[cfg(not(any(target_os = "linux", target_os = "freebsd", target_os = "netbsd")))]
  mod not_linux_like {
    use super::*;

    #[napi(js_name = "share")]
    pub fn share_js(payload: Buffer, name: Option<String>) -> napi::Result<Pipe> {
      share_shm(payload, name)
    }
  }
}

#[cfg(target_family = "windows")]
mod windows {
  use super::*;
  use std::ffi::OsStr;
  use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
  use std::os::windows::prelude::*;
  use std::sync::Arc;
  use std::sync::atomic::{AtomicBool, Ordering};
  use std::{ptr, thread};
  use ulid::Ulid;
  use windows_sys::Win32::{
    Foundation::{CloseHandle, ERROR_PIPE_CONNECTED, GetLastError, HANDLE, INVALID_HANDLE_VALUE},
    Storage::FileSystem::{FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_OUTBOUND},
    Storage::FileSystem::{FlushFileBuffers, WriteFile},
    System::Pipes::{
      ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE,
      PIPE_WAIT,
    },
  };
  use windows_sys::core::BOOL;

  #[napi]
  pub struct Pipe {
    path: String,
    should_stop: Arc<AtomicBool>,
  }

  #[napi]
  impl Pipe {
    #[napi(getter)]
    pub fn path(&self) -> String {
      self.path.clone()
    }

    #[napi]
    pub fn close(&self) -> Result<()> {
      self.should_stop.store(true, Ordering::Relaxed);
      Ok(())
    }
  }

  pub fn named_pipe(payload: Buffer, name_hint: Option<String>) -> Result<Pipe> {
    // Named pipe namespace is flat; keep it short-ish and unique
    let name = name_hint.unwrap_or_else(|| "share".to_string());
    let pipe_path = format!(r"\\.\pipe\{}-{}", name, Ulid::new().to_string());

    let wide_path: Vec<u16> = OsStr::new(&pipe_path)
      .encode_wide()
      .chain(std::iter::once(0))
      .collect();

    // Create one instance. Client will read bytes; we only need outbound from server.
    // PIPE_WAIT = blocking mode.
    let handle: HANDLE = unsafe {
      CreateNamedPipeW(
        wide_path.as_ptr(),
        PIPE_ACCESS_OUTBOUND | FILE_FLAG_FIRST_PIPE_INSTANCE,
        PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
        255,             // max instances
        64 * 1024,       // out buffer
        64 * 1024,       // in buffer (unused)
        0,               // default timeout
        ptr::null_mut(), // default security attrs
      )
    };

    if handle == INVALID_HANDLE_VALUE {
      return Err(last_err("CreateNamedPipeW failed"));
    }

    // Take ownership of the handle (so it will be closed on drop) and move it to the thread.
    let owned: OwnedHandle = unsafe { OwnedHandle::from_raw_handle(handle as RawHandle) };

    // Copy payload into thread-owned memory
    let bytes = payload.to_vec();

    let should_stop = Arc::new(AtomicBool::new(false));
    let should_stop_clone = should_stop.clone();

    // Serve in a background thread so we don't block Node's thread.
    thread::spawn(move || unsafe {
      let h: HANDLE = owned.as_raw_handle() as HANDLE;

      // Keep serving connections until told to stop
      while !should_stop_clone.load(Ordering::Relaxed) {
        // Wait for a client to connect
        let ok: BOOL = ConnectNamedPipe(h, ptr::null_mut());
        if ok == 0 {
          let err = GetLastError();
          if err != ERROR_PIPE_CONNECTED {
            // if should_stop_clone.load(Ordering::Relaxed) {
            //   break;
            // }
            continue;
          }
        }
        // Check again before writing
        if should_stop_clone.load(Ordering::Relaxed) {
          DisconnectNamedPipe(h);
          break;
        }

        // Write bytes to the connected client
        let mut written: u32 = 0;
        let _ = WriteFile(
          h,
          bytes.as_ptr() as _,
          bytes.len() as u32,
          &mut written as *mut u32,
          ptr::null_mut(),
        );

        // Flush and disconnect to allow next connection
        let _ = FlushFileBuffers(h);
        let _ = DisconnectNamedPipe(h);
      }

      // Cleanup when done
      let _ = CloseHandle(h);
    });

    Ok(Pipe {
      path: pipe_path.clone(),
      should_stop,
    })
  }

  #[napi(js_name = "shareNamedPipe")]
  pub fn share_named_pipe_js(payload: Buffer, name_hint: Option<String>) -> Result<Pipe> {
    named_pipe(payload, name_hint)
  }

  #[napi(js_name = "share")]
  pub fn share(payload: Buffer, name_hint: Option<String>) -> Result<Pipe> {
    named_pipe(payload, name_hint)
  }
}

#[cfg(test)]
mod tests {
  use ulid::Ulid;

  #[test]
  fn it_works() {
    let result = format!("/share-{}-{}", std::process::id(), Ulid::new().to_string());
    dbg!("{:?}", result);
    ///share-186352-01KF0MCZ1S4H80VP3A8JS5NA02
    assert_eq!(1, 2);
  }
}
