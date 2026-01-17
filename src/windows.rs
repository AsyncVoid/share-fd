use crate::{NamedPipe, Pipe, last_err};
use napi::bindgen_prelude::*;
use napi_derive::napi;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

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
  impl NamedPipe {
    #[napi(getter)]
    pub fn path(&self) -> String {
      self.path.clone()
    }

    #[napi]
    pub fn close(&self) -> () {
      self.should_stop.store(true, Ordering::Relaxed);
      unsafe {
        CloseHandle(self.handle.as_raw_handle() as HANDLE);
      }
    }
  }

  impl Drop for NamedPipe {
    fn drop(&mut self) {
      self.close()
    }
  }

  pub fn named_pipe(payload: Buffer, name_hint: Option<String>) -> Result<NamedPipe> {
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

    Ok(NamedPipe {
      path: pipe_path.clone(),
      should_stop,
      handle: owned.try_clone()?,
    })
  }

  #[napi(js_name = "shareNamedPipe")]
  pub fn share_named_pipe_js(payload: Buffer, name_hint: Option<String>) -> Result<NamedPipe> {
    named_pipe(payload, name_hint)
  }

  #[napi(js_name = "share")]
  pub fn share(payload: Buffer, name_hint: Option<String>) -> Result<NamedPipe> {
    named_pipe(payload, name_hint)
  }
}
