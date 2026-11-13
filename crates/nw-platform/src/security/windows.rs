//! Inspect the actual protected object through native Windows security APIs.
use super::{check_dacl, denied};
use crate::files::{diagnostic_io_error, native_object};
use nw_domain::Result;
use std::{
    ffi::c_void,
    fs::{File, OpenOptions},
    os::windows::{
        fs::{MetadataExt, OpenOptionsExt},
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::Path,
    ptr,
};
use windows_sys::Win32::{
    Foundation::{ERROR_INSUFFICIENT_BUFFER, LocalFree},
    Security::{
        Authorization::{
            ConvertSecurityDescriptorToStringSecurityDescriptorW,
            ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
            GetSecurityInfo, SDDL_REVISION_1, SE_FILE_OBJECT, SetSecurityInfo,
        },
        DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl, GetTokenInformation,
        PROTECTED_DACL_SECURITY_INFORMATION, TOKEN_QUERY, TOKEN_USER, TokenUser,
    },
    Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE,
        FILE_SHARE_READ, FILE_SHARE_WRITE, READ_CONTROL, WRITE_DAC,
    },
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};

/// Each pointer is either null or a unique LocalAlloc-family API output.
struct LocalMemory(*mut c_void);
#[expect(unsafe_code, reason = "Own and release Windows LocalAlloc-family security outputs")]
impl Drop for LocalMemory {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: The APIs below transfer these unique allocations to us.
            unsafe { LocalFree(self.0); }
        }
    }
}

fn last_error(operation: &'static str) -> nw_domain::DomainError {
    diagnostic_io_error(operation, std::io::Error::last_os_error())
}
fn status_error(operation: &'static str, status: u32) -> nw_domain::DomainError {
    diagnostic_io_error(operation, std::io::Error::from_raw_os_error(status as i32))
}

#[expect(unsafe_code, reason = "Read bounded TokenUser data and own the returned process token/SID text")]
fn current_sid() -> Result<String> {
    let mut token = ptr::null_mut();
    // SAFETY: Valid pseudo process handle and initialized output; no impersonation.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(last_error("acl.token_open"));
    }
    if token.is_null() { return Err(denied("Windows process token is unavailable")); }
    // SAFETY: OpenProcessToken succeeded and transferred this unique real handle.
    let token = unsafe { OwnedHandle::from_raw_handle(token) };
    let mut needed = 0;
    // SAFETY: The zero-buffer query writes only the required-size output.
    let query = unsafe {
        GetTokenInformation(token.as_raw_handle(), TokenUser, ptr::null_mut(), 0, &mut needed)
    };
    if query != 0 || std::io::Error::last_os_error().raw_os_error() != Some(ERROR_INSUFFICIENT_BUFFER as i32)
        || !(std::mem::size_of::<TOKEN_USER>() as u32..=32768).contains(&needed)
    {
        return Err(denied("Windows TokenUser size could not be established"));
    }
    // usize storage supplies TOKEN_USER's required pointer alignment.
    let mut buffer = vec![0usize; (needed as usize).div_ceil(std::mem::size_of::<usize>())];
    // SAFETY: Aligned, initialized storage covers the exact requested byte length.
    if unsafe {
        GetTokenInformation(token.as_raw_handle(), TokenUser, buffer.as_mut_ptr().cast(), needed, &mut needed)
    } == 0 { return Err(last_error("acl.token_read")); }
    // SAFETY: A successful TokenUser query initialized TOKEN_USER at this aligned address.
    let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
    let mut text = ptr::null_mut();
    // SAFETY: The SID belongs to the still-live TokenUser buffer; output is LocalAlloc-owned.
    if unsafe { ConvertSidToStringSidW(user.User.Sid, &mut text) } == 0 {
        return Err(last_error("acl.sid_text"));
    }
    let allocation = LocalMemory(text.cast());
    if allocation.0.is_null() { return Err(denied("Windows SID text is unavailable")); }
    // A valid Windows SID has at most 15 subauthorities and fits this text bound.
    for length in 0..256 {
        // SAFETY: ConvertSidToStringSidW returns valid NUL-terminated SID text.
        if unsafe { *text.add(length) } == 0 {
            // SAFETY: This is the initialized prefix preceding its terminator.
            let units = unsafe { std::slice::from_raw_parts(text, length) };
            return String::from_utf16(units).map_err(|error| diagnostic_io_error("acl.sid_decode", error));
        }
    }
    Err(denied("Windows SID text exceeds its bound"))
}

fn open_object(path: &Path, fresh: bool) -> Result<File> {
    let file = OpenOptions::new()
        .access_mode(READ_CONTROL | FILE_READ_ATTRIBUTES | if fresh { WRITE_DAC } else { 0 })
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|error| diagnostic_io_error("acl.object_open", error))?;
    let metadata = file.metadata().map_err(|error| diagnostic_io_error("acl.object_metadata", error))?;
    if (!metadata.is_dir() && !metadata.is_file())
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    { return Err(denied("Protected ACL object must be an ordinary file or directory")); }
    Ok(file)
}

#[expect(unsafe_code, reason = "Install a protected current-user DACL only on a newly created object")]
fn protect_new(file: &File, sid: &str) -> Result<()> {
    let text = format!("D:P(A;OICI;FA;;;{sid})").encode_utf16().chain(std::iter::once(0)).collect::<Vec<_>>();
    let mut descriptor = ptr::null_mut();
    // SAFETY: Valid NUL-terminated trusted SID descriptor and initialized output.
    let okay = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(text.as_ptr(), SDDL_REVISION_1, &mut descriptor, ptr::null_mut())
    };
    if okay == 0 { return Err(last_error("acl.descriptor_create")); }
    let allocation = LocalMemory(descriptor);
    if allocation.0.is_null() { return Err(denied("Protected security descriptor is absent")); }
    let mut present = 0;
    let mut defaulted = 0;
    let mut dacl = ptr::null_mut();
    // SAFETY: Descriptor allocation remains live; output ACL is borrowed from it.
    if unsafe { GetSecurityDescriptorDacl(allocation.0, &mut present, &mut dacl, &mut defaulted) } == 0 {
        return Err(last_error("acl.descriptor_dacl"));
    }
    if present == 0 || dacl.is_null() { return Err(denied("Protected DACL is absent")); }
    // SAFETY: Held file has WRITE_DAC; Windows copies the borrowed ACL synchronously.
    let status = unsafe {
        SetSecurityInfo(file.as_raw_handle(), SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            ptr::null_mut(), ptr::null_mut(), dacl, ptr::null())
    };
    if status != 0 { return Err(status_error("acl.dacl_install", status)); }
    Ok(())
}

#[expect(unsafe_code, reason = "Inspect the held object's actual DACL and copy its bounded native SDDL")]
fn descriptor_text(file: &File) -> Result<String> {
    let mut descriptor = ptr::null_mut();
    // SAFETY: The held handle has READ_CONTROL; all unused outputs are null.
    let status = unsafe {
        GetSecurityInfo(file.as_raw_handle(), SE_FILE_OBJECT, DACL_SECURITY_INFORMATION,
            ptr::null_mut(), ptr::null_mut(), ptr::null_mut(), ptr::null_mut(), &mut descriptor)
    };
    if status != 0 { return Err(status_error("acl.dacl_read", status)); }
    let allocation = LocalMemory(descriptor);
    if allocation.0.is_null() { return Err(denied("Protected security descriptor is absent")); }
    let mut text = ptr::null_mut();
    let mut length = 0;
    // SAFETY: The descriptor remains live and outputs receive a unique LocalAlloc string.
    let okay = unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(allocation.0, SDDL_REVISION_1,
            DACL_SECURITY_INFORMATION, &mut text, &mut length)
    };
    if okay == 0 { return Err(last_error("acl.dacl_text")); }
    let text_allocation = LocalMemory(text.cast());
    if text_allocation.0.is_null() || !(1..=256 * 1024).contains(&length) {
        return Err(denied("Windows ACL text exceeds its bound"));
    }
    // SAFETY: The API reports its initialized UTF-16 length including the terminator.
    let units = unsafe { std::slice::from_raw_parts(text, length as usize) };
    if units.last() != Some(&0) { return Err(denied("Windows ACL text is unterminated")); }
    String::from_utf16(&units[..units.len() - 1])
        .map_err(|error| diagnostic_io_error("acl.dacl_decode", error))
}

pub(super) fn protect(path: &Path, fresh: bool) -> Result<()> {
    let sid = current_sid()?;
    let file = open_object(path, fresh)?;
    if fresh { protect_new(&file, &sid)?; }
    check_dacl(&descriptor_text(&file)?, &sid)?;
    // The checked handle must still be the object named by the caller's path.
    let current = open_object(path, false)?;
    if native_object(&file)? != native_object(&current)? {
        return Err(denied("Protected ACL object changed during verification"));
    }
    Ok(())
}
