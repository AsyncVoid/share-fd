use crate::{NamedPipe, Pipe, last_err};
use napi::bindgen_prelude::*;
use napi_derive::napi;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(target_family = "unix")]
mod unix {
  use super::*;
  use napi::Error;
  use std::ffi::{CString, c_void};
  use std::os::fd::RawFd;
  use std::sync::Arc;
  use std::sync::atomic::{AtomicBool, Ordering};
  use std::{cmp, thread};
  use ulid::Ulid;

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

  impl Drop for Pipe {
    fn drop(&mut self) {
      unsafe {
        libc::close(self.fd);
      }
    }
  }

  #[napi]
  impl NamedPipe {
    #[napi(getter)]
    pub fn path(&self) -> String {
      self.path.clone()
    }

    #[napi]
    pub fn close(&self) -> Result<()> {
      self.should_stop.store(true, Ordering::Relaxed);

      if !self.is_closed.load(Ordering::Relaxed) {
        let cpath = CString::new(self.path.as_str())?;
        unsafe {
          if libc::unlink(cpath.as_ptr()) == 0 {
            self.is_closed.store(true, Ordering::Relaxed);
          } else {
            return Err(last_err("unlink failed"));
          }
        }
      }
      Ok(())
    }
  }

  impl Drop for NamedPipe {
    fn drop(&mut self) {
      self.close().unwrap()
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

  #[cfg(not(any(target_os = "linux", target_os = "freebsd", target_os = "netbsd")))]
  #[napi(js_name = "shareMemFD")]
  pub fn share_memfd_js(payload: Buffer, name: Option<String>) -> Result<Pipe> {
    Err(Error::from_reason("Unsupported platform"))
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

    let bytes = payload.to_vec();

    let ftrunc = unsafe {
      if cfg!(target_os = "macos") {
        libc::ftruncate(fd, bytes.len() as i64 + 1)
      } else {
        libc::ftruncate(fd, 0)
      }
    };

    if ftrunc < 0 {
      return Err(last_err("ftruncate failed"));
    }

    #[cfg(target_os = "macos")]
    unsafe {
      let addr = libc::mmap(
        core::ptr::null_mut(), // !0 as *mut c_void,
        bytes.len() + 1,
        libc::PROT_READ | libc::PROT_WRITE,
        libc::MAP_SHARED,
        fd,
        0,
      );

      if addr == libc::MAP_FAILED {
        return Err(last_err("mmap failed"));
      }

      libc::memcpy(addr, bytes.as_ptr() as *const libc::c_void, bytes.len() + 1) as isize
    };

    #[cfg(not(target_os = "macos"))]
    unsafe {
      let write = unsafe { libc::write(fd, bytes.as_ptr() as *const libc::c_void, bytes.len()) };

      if write < 0 {
        return Err(crate::last_err("write failed"));
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

  #[cfg(not(any(
    target_os = "linux",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "macos"
  )))]
  #[napi(js_name = "share")]
  pub fn share(payload: Buffer, name: Option<String>) -> napi::Result<Pipe> {
    share_shm(payload, name)
  }

  async fn share_fifo(payload: Buffer, name: Option<String>) -> Result<NamedPipe> {
    let name = name.unwrap_or_else(|| Ulid::new().to_string());
    let path = format!("/tmp/share-{}", name);

    let cpath = CString::new(path.as_str()).map_err(|_| Error::from_reason("path contains NUL"))?;

    unsafe {
      if libc::mkfifo(cpath.as_ptr(), 0o600) < 0 {
        return Err(last_err("mkfifo failed"));
      }
    }

    let bytes = payload.to_vec();

    let should_stop = Arc::new(AtomicBool::new(false));
    let should_stop_clone = Arc::clone(&should_stop);
    let is_closed = Arc::new(AtomicBool::new(false));
    let is_closed_clone = Arc::clone(&is_closed);

    // let handle = tokio::runtime::Handle::try_current()
    //     .or_else(|_| {
    //       println!("Creating runtime");
    //       // Fallback: create runtime if not in tokio context
    //       tokio::runtime::Runtime::new().map(|rt| rt.handle().clone())
    //     })
    //     .map_err(|e| Error::from_reason(format!("tokio runtime error: {}", e)))?;

    // Background thread to serve connections
    tokio::spawn(async move {
      let bytes = Arc::new(bytes);
      let cpath = Arc::new(cpath);

      while !should_stop_clone.load(Ordering::Relaxed) {
        let cpath = Arc::clone(&cpath);
        let bytes = Arc::clone(&bytes);

        let result = tokio::task::spawn_blocking(move || {
          // Open FIFO for writing (blocks until reader connects)
          let fd = unsafe { libc::open(cpath.clone().as_ptr(), libc::O_WRONLY) };

          if fd < 0 {
            return;
          }

          // Write payload
          unsafe {
            libc::write(fd, bytes.as_ptr() as *const c_void, bytes.len());
            libc::close(fd);
          }
        }).await;

        if result.is_err() || should_stop_clone.load(Ordering::Relaxed) {
          break;
        }
      }

      // Cleanup
      if !is_closed_clone.load(Ordering::Relaxed) {
        unsafe {
          if libc::unlink(cpath.as_ptr()) == 0 {
            is_closed_clone.store(true, Ordering::Relaxed);
          }
        }
      }
    });

    Ok(NamedPipe { path, should_stop, is_closed })
  }

  #[napi(js_name = "shareFIFO")]
  pub async fn share_fifo_js(payload: Buffer, name: Option<String>) -> Result<NamedPipe> {
    share_fifo(payload, name).await
  }

  #[napi(js_name = "shareNamedPipe")]
  pub async fn share_named_pipe(payload: Buffer, name: Option<String>) -> Result<NamedPipe> {
    share_fifo(payload, name).await
  }

  #[cfg(target_os = "macos")]
  #[napi(js_name = "share")]
  pub async fn share(payload: Buffer, name: Option<String>) -> Result<Either<NamedPipe, Pipe>> {
    Ok(Either::A(share_fifo(payload, name).await?))
  }

  fn pipe(payload: Buffer, name: Option<String>) -> Result<Pipe> {
    let name = name.unwrap_or_else(|| "pipe".to_string());
    let mut fds: [RawFd; 2] = [0; 2];

    unsafe {
      if libc::pipe(fds.as_mut_ptr()) < 0 {
        return Err(last_err("pipe failed"));
      }
    }

    let read_fd = fds[0];
    let write_fd = fds[1];

    let bytes = payload.to_vec();

    unsafe {
      let written = libc::write(write_fd, bytes.as_ptr() as *const c_void, bytes.len());
      if written < 0 {
        libc::close(read_fd);
        libc::close(write_fd);
        return Err(last_err("write failed"));
      }

      // Close write end - signals EOF to reader
      libc::close(write_fd);
    }

    Ok(Pipe { fd: read_fd, name })
  }

  #[napi(js_name = "sharePipe")]
  pub fn share_pipe_js(payload: Buffer, name: Option<String>) -> Result<Pipe> {
    pipe(payload, name)
  }
}
