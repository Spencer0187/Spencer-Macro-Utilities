//! Windows `WinINet` transport for updater release/manifest checks.

use crate::{MAXIMUM_UPDATE_BYTES, UpdateTransport, UpdateTransportError};
use std::ffi::c_void;
use windows_sys::Win32::Networking::WinInet::{
    HTTP_QUERY_FLAG_NUMBER, HTTP_QUERY_STATUS_CODE, HttpQueryInfoW, INTERNET_FLAG_NO_CACHE_WRITE,
    INTERNET_FLAG_NO_UI, INTERNET_FLAG_RELOAD, INTERNET_FLAG_SECURE, INTERNET_OPEN_TYPE_DIRECT,
    INTERNET_OPTION_CONNECT_TIMEOUT, INTERNET_OPTION_RECEIVE_TIMEOUT, InternetCloseHandle,
    InternetOpenUrlW, InternetOpenW, InternetReadFile, InternetSetOptionW,
};

const TIMEOUT_MS: u32 = 10_000;
const READ_BUFFER_BYTES: usize = 8192;
const REQUEST_HEADERS: &str =
    "User-Agent: Spencer-Macro-Utilities-Updater\r\nAccept: application/vnd.github+json\r\n";

#[derive(Debug)]
struct InternetHandle(*mut c_void);

impl InternetHandle {
    fn new(handle: *mut c_void) -> Option<Self> {
        (!handle.is_null()).then_some(Self(handle))
    }

    const fn raw(&self) -> *mut c_void {
        self.0
    }
}

impl Drop for InternetHandle {
    fn drop(&mut self) {
        // SAFETY: this wrapper uniquely owns a non-null WinINet handle.
        unsafe {
            let _ = InternetCloseHandle(self.0);
        }
    }
}

fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn set_timeouts(internet: &InternetHandle) {
    let timeout = TIMEOUT_MS;
    let timeout_size = u32::try_from(std::mem::size_of_val(&timeout)).unwrap_or_default();
    // SAFETY: `timeout` remains live through each synchronous call and the handle is valid.
    unsafe {
        let _ = InternetSetOptionW(
            internet.raw(),
            INTERNET_OPTION_CONNECT_TIMEOUT,
            (&raw const timeout).cast(),
            timeout_size,
        );
        let _ = InternetSetOptionW(
            internet.raw(),
            INTERNET_OPTION_RECEIVE_TIMEOUT,
            (&raw const timeout).cast(),
            timeout_size,
        );
    }
}

fn query_success_status(connection: &InternetHandle) -> bool {
    let mut status_code = 0_u32;
    let mut status_size = u32::try_from(std::mem::size_of_val(&status_code)).unwrap_or_default();
    // SAFETY: the status buffer/size pointer are writable for the duration of the synchronous call.
    let queried = unsafe {
        HttpQueryInfoW(
            connection.raw(),
            HTTP_QUERY_STATUS_CODE | HTTP_QUERY_FLAG_NUMBER,
            (&raw mut status_code).cast(),
            &raw mut status_size,
            std::ptr::null_mut(),
        )
    };
    queried != 0 && (200..300).contains(&status_code)
}

fn read_response(connection: &InternetHandle) -> Result<Vec<u8>, UpdateTransportError> {
    let mut data = Vec::new();
    let mut buffer = [0_u8; READ_BUFFER_BYTES];
    loop {
        let mut bytes_read = 0_u32;
        // SAFETY: the buffer and byte-count pointer are valid writable storage for this call.
        let read_ok = unsafe {
            InternetReadFile(
                connection.raw(),
                buffer.as_mut_ptr().cast(),
                u32::try_from(buffer.len()).unwrap_or_default(),
                &raw mut bytes_read,
            )
        };
        if read_ok == 0 {
            return Err(UpdateTransportError::new(
                "WinINet failed while reading the update response.",
            ));
        }
        if bytes_read == 0 {
            break;
        }
        let count = usize::try_from(bytes_read).unwrap_or(usize::MAX);
        if count > buffer.len()
            || data.len() > usize::try_from(MAXIMUM_UPDATE_BYTES).unwrap_or(usize::MAX) - count
        {
            return Err(UpdateTransportError::new(
                "Updater download exceeded the 512 MiB safety limit.",
            ));
        }
        data.extend_from_slice(&buffer[..count]);
    }
    if data.is_empty() {
        return Err(UpdateTransportError::new(String::new()));
    }
    Ok(data)
}

fn download_url_to_memory(url: &str) -> Result<Vec<u8>, UpdateTransportError> {
    if !url.starts_with("https://") {
        return Err(UpdateTransportError::new("Updater refused a non-HTTPS URL."));
    }
    let url = wide_null(url);
    let agent = wide_null("Spencer-Macro-Utilities-Updater");
    let headers = wide_null(REQUEST_HEADERS);

    // SAFETY: all strings are NUL-terminated and remain live through the synchronous call.
    let internet = unsafe {
        InternetOpenW(
            agent.as_ptr(),
            INTERNET_OPEN_TYPE_DIRECT,
            std::ptr::null(),
            std::ptr::null(),
            0,
        )
    };
    let Some(internet) = InternetHandle::new(internet) else {
        return Err(UpdateTransportError::new("WinINet InternetOpen failed."));
    };
    set_timeouts(&internet);

    // SAFETY: the WinINet session and NUL-terminated buffers stay live through this call.
    let connection = unsafe {
        InternetOpenUrlW(
            internet.raw(),
            url.as_ptr(),
            headers.as_ptr(),
            u32::MAX,
            INTERNET_FLAG_RELOAD
                | INTERNET_FLAG_NO_CACHE_WRITE
                | INTERNET_FLAG_SECURE
                | INTERNET_FLAG_NO_UI,
            0,
        )
    };
    let Some(connection) = InternetHandle::new(connection) else {
        return Err(UpdateTransportError::new("WinINet could not open the update URL."));
    };
    if !query_success_status(&connection) {
        return Err(UpdateTransportError::new(
            "Updater server returned a non-success HTTP status.",
        ));
    }
    read_response(&connection)
}

#[derive(Clone, Copy, Debug, Default)]
pub struct WinInetUpdateTransport;

impl UpdateTransport for WinInetUpdateTransport {
    fn get(&mut self, url: &str) -> Result<Vec<u8>, UpdateTransportError> {
        download_url_to_memory(url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_https_before_touching_wininet() {
        assert_eq!(
            WinInetUpdateTransport.get("http://example.invalid/update").unwrap_err().to_string(),
            "Updater refused a non-HTTPS URL."
        );
    }

    #[test]
    fn request_headers_match_cpp_transport() {
        assert_eq!(
            REQUEST_HEADERS,
            "User-Agent: Spencer-Macro-Utilities-Updater\r\nAccept: application/vnd.github+json\r\n"
        );
        assert_eq!(TIMEOUT_MS, 10_000);
        assert_eq!(READ_BUFFER_BYTES, 8192);
    }
}
