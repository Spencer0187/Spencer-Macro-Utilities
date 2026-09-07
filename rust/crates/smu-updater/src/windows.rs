//! Windows `WinINet` transport for updater release/manifest checks.

use crate::{MAXIMUM_UPDATE_BYTES, UpdateTransport, UpdateTransportError};
use std::ffi::c_void;
use std::fmt;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use windows_sys::Win32::{
    Networking::WinInet::{
        HTTP_QUERY_FLAG_NUMBER, HTTP_QUERY_STATUS_CODE, HttpQueryInfoW,
        INTERNET_FLAG_NO_CACHE_WRITE, INTERNET_FLAG_NO_UI, INTERNET_FLAG_RELOAD,
        INTERNET_FLAG_SECURE, INTERNET_OPEN_TYPE_DIRECT, INTERNET_OPTION_CONNECT_TIMEOUT,
        INTERNET_OPTION_RECEIVE_TIMEOUT, InternetCloseHandle, InternetOpenUrlW, InternetOpenW,
        InternetReadFile, InternetSetOptionW,
    },
    Security::{
        Cryptography::{
            CERT_CONTEXT, CERT_FIND_SUBJECT_CERT, CERT_INFO,
            CERT_QUERY_CONTENT_FLAG_PKCS7_SIGNED_EMBED, CERT_QUERY_FORMAT_FLAG_BINARY,
            CERT_QUERY_OBJECT_FILE, CERT_SHA256_HASH_PROP_ID, CMSG_SIGNER_INFO,
            CMSG_SIGNER_INFO_PARAM, CertCloseStore, CertFindCertificateInStore,
            CertFreeCertificateContext, CertGetCertificateContextProperty, CryptMsgClose,
            CryptMsgGetParam, CryptQueryObject, HCERTSTORE, PKCS_7_ASN_ENCODING, X509_ASN_ENCODING,
        },
        WinTrust::{
            WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_DATA_0, WINTRUST_FILE_INFO,
            WTD_CHOICE_FILE, WTD_REVOCATION_CHECK_CHAIN, WTD_REVOKE_WHOLECHAIN,
            WTD_STATEACTION_CLOSE, WTD_STATEACTION_VERIFY, WTD_UI_NONE, WinVerifyTrust,
        },
    },
    Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VS_FIXEDFILEINFO, VerQueryValueW,
    },
};

const TIMEOUT_MS: u32 = 10_000;
const READ_BUFFER_BYTES: usize = 8192;
const REQUEST_HEADERS: &str =
    "User-Agent: Spencer-Macro-Utilities-Updater\r\nAccept: application/vnd.github+json\r\n";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowsUpdateTrustError(String);

impl WindowsUpdateTrustError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for WindowsUpdateTrustError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for WindowsUpdateTrustError {}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct WindowsFileVersion {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
    pub revision: u16,
}

impl WindowsFileVersion {
    #[must_use]
    pub const fn as_tuple(self) -> (u16, u16, u16, u16) {
        (self.major, self.minor, self.patch, self.revision)
    }
}

#[derive(Debug)]
struct CertStoreHandle(HCERTSTORE);

impl Drop for CertStoreHandle {
    fn drop(&mut self) {
        // SAFETY: this wrapper uniquely owns a certificate store returned by CryptQueryObject.
        unsafe {
            let _ = CertCloseStore(self.0, 0);
        }
    }
}

#[derive(Debug)]
struct CryptMessageHandle(*mut c_void);

impl Drop for CryptMessageHandle {
    fn drop(&mut self) {
        // SAFETY: this wrapper uniquely owns a cryptographic message returned by CryptQueryObject.
        unsafe {
            let _ = CryptMsgClose(self.0);
        }
    }
}

#[derive(Debug)]
struct CertContextHandle(*mut CERT_CONTEXT);

impl Drop for CertContextHandle {
    fn drop(&mut self) {
        // SAFETY: this wrapper uniquely owns a certificate context returned by CertFindCertificateInStore.
        unsafe {
            let _ = CertFreeCertificateContext(self.0);
        }
    }
}

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

fn wide_path(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
}

fn ffi_size_u32<T>() -> u32 {
    u32::try_from(std::mem::size_of::<T>()).unwrap_or_default()
}

/// Read the numeric `FILEVERSION` embedded in a Windows executable.
///
/// # Errors
///
/// Returns an error when the file is missing, has no readable version resource, or exposes malformed
/// `VS_FIXEDFILEINFO` data.
pub fn windows_file_version(path: &Path) -> Result<WindowsFileVersion, WindowsUpdateTrustError> {
    if !path.is_file() {
        return Err(WindowsUpdateTrustError::new("Windows version-info target does not exist."));
    }
    let path = wide_path(path);
    let mut ignored = 0_u32;
    // SAFETY: the NUL-terminated path and writable handle value remain valid for the synchronous call.
    let size = unsafe { GetFileVersionInfoSizeW(path.as_ptr(), &raw mut ignored) };
    if size == 0 {
        return Err(WindowsUpdateTrustError::new(
            "Could not read the Windows executable version resource.",
        ));
    }
    let mut data = vec![0_u8; usize::try_from(size).unwrap_or_default()];
    if data.is_empty() {
        return Err(WindowsUpdateTrustError::new(
            "Could not allocate the Windows executable version resource buffer.",
        ));
    }
    // SAFETY: `data` is writable for `size` bytes and the path remains live for this synchronous call.
    if unsafe { GetFileVersionInfoW(path.as_ptr(), 0, size, data.as_mut_ptr().cast()) } == 0 {
        return Err(WindowsUpdateTrustError::new(
            "Could not load the Windows executable version resource.",
        ));
    }
    let root = wide_null("\\");
    let mut value = std::ptr::null_mut();
    let mut value_size = 0_u32;
    // SAFETY: the version-resource buffer and NUL-terminated root query stay live through this call.
    let queried = unsafe {
        VerQueryValueW(data.as_ptr().cast(), root.as_ptr(), &raw mut value, &raw mut value_size)
    };
    if queried == 0
        || value.is_null()
        || usize::try_from(value_size).unwrap_or_default() < std::mem::size_of::<VS_FIXEDFILEINFO>()
    {
        return Err(WindowsUpdateTrustError::new(
            "Windows executable version resource did not contain VS_FIXEDFILEINFO.",
        ));
    }
    // SAFETY: VerQueryValueW returned at least sizeof(VS_FIXEDFILEINFO) bytes. `read_unaligned` avoids
    // assuming the pointer returned into the byte buffer has Rust's native struct alignment.
    let info = unsafe { std::ptr::read_unaligned(value.cast::<VS_FIXEDFILEINFO>()) };
    if info.dwSignature != 0xFEEF_04BD || info.dwStrucVersion != 0x0001_0000 {
        return Err(WindowsUpdateTrustError::new(
            "Windows executable version resource had an invalid fixed-info header.",
        ));
    }
    Ok(WindowsFileVersion {
        major: u16::try_from(info.dwFileVersionMS >> 16).unwrap_or_default(),
        minor: u16::try_from(info.dwFileVersionMS & 0xFFFF).unwrap_or_default(),
        patch: u16::try_from(info.dwFileVersionLS >> 16).unwrap_or_default(),
        revision: u16::try_from(info.dwFileVersionLS & 0xFFFF).unwrap_or_default(),
    })
}

/// Validate an executable's embedded Authenticode signature and certificate chain.
///
/// # Errors
///
/// Returns an error when the file is missing or WinTrust rejects the signature/chain/revocation state.
pub fn verify_authenticode_signature(path: &Path) -> Result<(), WindowsUpdateTrustError> {
    if !path.is_file() {
        return Err(WindowsUpdateTrustError::new(
            "Authenticode verification target does not exist.",
        ));
    }
    let path = wide_path(path);
    let mut file_info = WINTRUST_FILE_INFO {
        cbStruct: ffi_size_u32::<WINTRUST_FILE_INFO>(),
        pcwszFilePath: path.as_ptr(),
        hFile: std::ptr::null_mut(),
        pgKnownSubject: std::ptr::null_mut(),
    };
    let mut trust_data = WINTRUST_DATA {
        cbStruct: ffi_size_u32::<WINTRUST_DATA>(),
        pPolicyCallbackData: std::ptr::null_mut(),
        pSIPClientData: std::ptr::null_mut(),
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_WHOLECHAIN,
        dwUnionChoice: WTD_CHOICE_FILE,
        Anonymous: WINTRUST_DATA_0 { pFile: &raw mut file_info },
        dwStateAction: WTD_STATEACTION_VERIFY,
        hWVTStateData: std::ptr::null_mut(),
        pwszURLReference: std::ptr::null_mut(),
        dwProvFlags: WTD_REVOCATION_CHECK_CHAIN,
        dwUIContext: 0,
        pSignatureSettings: std::ptr::null_mut(),
    };
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    // SAFETY: all WinTrust structures and the NUL-terminated path remain valid through the synchronous
    // verification call. WinVerifyTrust may populate hWVTStateData, which is closed below.
    let status = unsafe {
        WinVerifyTrust(std::ptr::null_mut(), &raw mut action, (&raw mut trust_data).cast())
    };
    trust_data.dwStateAction = WTD_STATEACTION_CLOSE;
    // SAFETY: this closes any WinTrust state allocated by the verification call above.
    unsafe {
        let _ = WinVerifyTrust(std::ptr::null_mut(), &raw mut action, (&raw mut trust_data).cast());
    }
    if status != 0 {
        return Err(WindowsUpdateTrustError::new(format!(
            "Authenticode verification failed with status 0x{:08X}.",
            u32::from_ne_bytes(status.to_ne_bytes())
        )));
    }
    Ok(())
}

fn query_signer_store_and_message(
    path: &Path,
) -> Result<(CertStoreHandle, CryptMessageHandle), WindowsUpdateTrustError> {
    let path = wide_path(path);
    let mut encoding = 0_u32;
    let mut content_type = 0_u32;
    let mut format_type = 0_u32;
    let mut store = std::ptr::null_mut();
    let mut message = std::ptr::null_mut();
    // SAFETY: the NUL-terminated path and all output pointers remain valid for the synchronous query.
    let queried = unsafe {
        CryptQueryObject(
            CERT_QUERY_OBJECT_FILE,
            path.as_ptr().cast(),
            CERT_QUERY_CONTENT_FLAG_PKCS7_SIGNED_EMBED,
            CERT_QUERY_FORMAT_FLAG_BINARY,
            0,
            &raw mut encoding,
            &raw mut content_type,
            &raw mut format_type,
            &raw mut store,
            &raw mut message,
            std::ptr::null_mut(),
        )
    };
    if queried == 0 || store.is_null() || message.is_null() {
        if !message.is_null() {
            // SAFETY: CryptQueryObject returned this message handle and ownership has not transferred.
            unsafe {
                let _ = CryptMsgClose(message);
            }
        }
        if !store.is_null() {
            // SAFETY: CryptQueryObject returned this store handle and ownership has not transferred.
            unsafe {
                let _ = CertCloseStore(store, 0);
            }
        }
        return Err(WindowsUpdateTrustError::new("Could not inspect the Authenticode signer."));
    }
    Ok((CertStoreHandle(store), CryptMessageHandle(message)))
}

fn signer_info_buffer(message: &CryptMessageHandle) -> Result<Vec<usize>, WindowsUpdateTrustError> {
    let mut bytes = 0_u32;
    // SAFETY: the size output pointer is valid and the message handle is live.
    let sized = unsafe {
        CryptMsgGetParam(message.0, CMSG_SIGNER_INFO_PARAM, 0, std::ptr::null_mut(), &raw mut bytes)
    };
    if sized == 0
        || usize::try_from(bytes).unwrap_or_default() < std::mem::size_of::<CMSG_SIGNER_INFO>()
    {
        return Err(WindowsUpdateTrustError::new("Could not inspect the Authenticode signer."));
    }
    let bytes_usize = usize::try_from(bytes).unwrap_or_default();
    let words = bytes_usize.div_ceil(std::mem::size_of::<usize>());
    let mut buffer = vec![0_usize; words];
    // SAFETY: the aligned word buffer is at least `bytes` long and writable; the message handle is live.
    let decoded = unsafe {
        CryptMsgGetParam(
            message.0,
            CMSG_SIGNER_INFO_PARAM,
            0,
            buffer.as_mut_ptr().cast(),
            &raw mut bytes,
        )
    };
    if decoded == 0 {
        return Err(WindowsUpdateTrustError::new("Could not decode the Authenticode signer."));
    }
    Ok(buffer)
}

/// Return the SHA-256 fingerprint of the certificate that signed an Authenticode file.
///
/// # Errors
///
/// Returns an error when the embedded signer cannot be decoded, located, or fingerprinted.
pub fn authenticode_signer_sha256(path: &Path) -> Result<Vec<u8>, WindowsUpdateTrustError> {
    if !path.is_file() {
        return Err(WindowsUpdateTrustError::new("Authenticode signer target does not exist."));
    }
    let (store, message) = query_signer_store_and_message(path)?;
    let signer_buffer = signer_info_buffer(&message)?;
    // SAFETY: signer_info_buffer allocates with usize alignment and retains the buffer for this scope.
    let signer = unsafe { &*signer_buffer.as_ptr().cast::<CMSG_SIGNER_INFO>() };
    let mut certificate_info = CERT_INFO::default();
    certificate_info.Issuer = signer.Issuer;
    certificate_info.SerialNumber = signer.SerialNumber;
    // SAFETY: signer-owned Issuer/Serial pointers remain valid while signer_buffer is live. The returned
    // certificate context receives its own store-managed lifetime and is wrapped immediately.
    let certificate = unsafe {
        CertFindCertificateInStore(
            store.0,
            X509_ASN_ENCODING | PKCS_7_ASN_ENCODING,
            0,
            CERT_FIND_SUBJECT_CERT,
            (&raw const certificate_info).cast(),
            std::ptr::null(),
        )
    };
    if certificate.is_null() {
        return Err(WindowsUpdateTrustError::new(
            "Could not locate the Authenticode signer certificate.",
        ));
    }
    let certificate = CertContextHandle(certificate);
    let mut fingerprint_size = 0_u32;
    // SAFETY: the certificate context is live and the size output pointer is valid.
    let sized = unsafe {
        CertGetCertificateContextProperty(
            certificate.0,
            CERT_SHA256_HASH_PROP_ID,
            std::ptr::null_mut(),
            &raw mut fingerprint_size,
        )
    };
    if sized == 0 || fingerprint_size == 0 {
        return Err(WindowsUpdateTrustError::new(
            "Could not calculate the Authenticode signer fingerprint.",
        ));
    }
    let mut fingerprint = vec![0_u8; usize::try_from(fingerprint_size).unwrap_or_default()];
    if fingerprint.is_empty() {
        return Err(WindowsUpdateTrustError::new(
            "Could not calculate the Authenticode signer fingerprint.",
        ));
    }
    // SAFETY: the fingerprint buffer is writable for fingerprint_size bytes and the certificate is live.
    let read = unsafe {
        CertGetCertificateContextProperty(
            certificate.0,
            CERT_SHA256_HASH_PROP_ID,
            fingerprint.as_mut_ptr().cast(),
            &raw mut fingerprint_size,
        )
    };
    if read == 0 {
        return Err(WindowsUpdateTrustError::new(
            "Could not calculate the Authenticode signer fingerprint.",
        ));
    }
    fingerprint.truncate(usize::try_from(fingerprint_size).unwrap_or_default());
    if fingerprint.is_empty() {
        return Err(WindowsUpdateTrustError::new(
            "Could not calculate the Authenticode signer fingerprint.",
        ));
    }
    Ok(fingerprint)
}

/// Require two signed executables to use the same Authenticode signer certificate.
///
/// # Errors
///
/// Returns an error when either signer cannot be read or their SHA-256 certificate fingerprints differ.
pub fn require_matching_authenticode_signer(
    current_executable: &Path,
    downloaded_executable: &Path,
) -> Result<(), WindowsUpdateTrustError> {
    verify_authenticode_signature(current_executable)?;
    verify_authenticode_signature(downloaded_executable)?;
    let current = authenticode_signer_sha256(current_executable)?;
    let downloaded = authenticode_signer_sha256(downloaded_executable)?;
    if current != downloaded {
        return Err(WindowsUpdateTrustError::new(
            "Downloaded update was signed by a different certificate than the running SMU executable.",
        ));
    }
    Ok(())
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

    #[test]
    fn authenticode_helpers_reject_missing_files_before_system_calls() {
        let missing = Path::new("smu-updater-authenticode-missing.exe");
        assert!(verify_authenticode_signature(missing).is_err());
        assert!(authenticode_signer_sha256(missing).is_err());
        assert!(require_matching_authenticode_signer(missing, missing).is_err());
    }
}
