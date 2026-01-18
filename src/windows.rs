use crate::{NamedPipe, Pipe, last_err};
use napi::bindgen_prelude::*;
use napi_derive::napi;
use std::ffi::OsStr;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
use std::os::windows::prelude::*;
use std::ptr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
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
      // dbg!("Closing...");
      CloseHandle(self.handle.as_raw_handle() as HANDLE);
      // dbg!("Closed");
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
  let pipe_path = pipe_path[..std::cmp::min(pipe_path.len(), 255)].to_string();

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
      //PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
      PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
      255,             // max instances
      64 * 1024,       // out buffer
      0,               // 64 * 1024,       // in buffer (unused)
      0,               // default timeout
      ptr::null_mut(), // default security attrs
    )
  };

  if handle == INVALID_HANDLE_VALUE {
    return Err(last_err("CreateNamedPipeW failed"));
  }

  // Take ownership of the handle (so it will be closed on drop) and move it to the thread.
  let owned: OwnedHandle = unsafe { OwnedHandle::from_raw_handle(handle as RawHandle) };
  let shared = Arc::new(owned);
  let thread_handle = Arc::clone(&shared);

  // Copy payload into thread-owned memory
  let bytes = payload.to_vec();

  let should_stop = Arc::new(AtomicBool::new(false));
  let should_stop_clone = should_stop.clone();
  let is_closed = Arc::new(AtomicBool::new(false));
  let is_closed_clone = Arc::clone(&is_closed);

  tokio::task::spawn(async move {
    let h = thread_handle;
    let bytes = Arc::new(bytes);

    // Keep serving connections until told to stop
    while !should_stop_clone.load(Ordering::Relaxed) {
      let h = h.clone();
      let bytes = Arc::clone(&bytes);
      let should_stop_clone = should_stop_clone.clone();

      // dbg!("Using blocking...");

      let result = tokio::task::spawn_blocking(move || {
        let h = h.as_raw_handle() as HANDLE;

        // dbg!("Waiting for client...");
        // Wait for a client to connect
        let ok: BOOL = unsafe { ConnectNamedPipe(h, ptr::null_mut()) };
        // dbg!("Connected");

        if ok == 0 {
          let err = unsafe { GetLastError() };
          if err != ERROR_PIPE_CONNECTED {
            // dbg!("Failed to connect: {}", err);
            return;
          }
        }

        // dbg!("Writing...");
        unsafe {
          // Write bytes to the connected client
          let mut written: u32 = 0;
          let _ = WriteFile(
            h,
            bytes.as_ptr() as _,
            bytes.len() as u32,
            &mut written as *mut u32,
            ptr::null_mut(),
          );

          // dbg!("Written {} bytes", written);

          // Flush and disconnect to allow next connection
          if FlushFileBuffers(h) == 0 {
            last_err("FlushFileBuffers failed");
          }

          // Doesn't cause EOF, problematic
          // if DisconnectNamedPipe(h) == 0 {
          //   last_err("DisconnectNamedPipe failed");
          // }
          // dbg!("Disconnected");

          // Needed to cause EOF, makes it one-shot
          CloseHandle(h);
          should_stop_clone.store(true, Ordering::Relaxed);
          return;
        }
      })
      .await;
    }

    // dbg!("Stopped");
  });

  Ok(NamedPipe {
    path: pipe_path.clone(),
    should_stop,
    handle: shared,
    is_closed,
  })
}

#[napi(js_name = "shareNamedPipe")]
pub async fn share_named_pipe_js(payload: Buffer, name_hint: Option<String>) -> Result<NamedPipe> {
  named_pipe(payload, name_hint)
}

#[napi(js_name = "share")]
pub async fn share(payload: Buffer, name_hint: Option<String>) -> Result<Either<NamedPipe, Pipe>> {
  Ok(Either::A(named_pipe(payload, name_hint)?))
}
