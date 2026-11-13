//! Inspect the actual protected object through native Windows security APIs.
use super::{check_dacl, denied};
use crate::files::{diagnostic_io_error, native_object};
use nw_domain::Result;
use std::{
    ffi::c_void,
    fs::{self, File, OpenOptions},
    os::windows::{
        ffi::OsStrExt,
        fs::{MetadataExt, OpenOptionsExt},
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::Path,
    ptr,
};
use windows_sys::Win32::{
    Foundation::{ERROR_ALREADY_EXISTS, ERROR_INSUFFICIENT_BUFFER, LocalFree},
    Security::{
        Authorization::{
            ConvertSecurityDescriptorToStringSecurityDescriptorW, ConvertSidToStringSidW,
            ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo, SDDL_REVISION_1,
            SE_FILE_OBJECT,
        },
        DACL_SECURITY_INFORMATION, GetTokenInformation, SECURITY_ATTRIBUTES, TOKEN_QUERY,
        TOKEN_USER, TokenUser,
    },
    Storage::FileSystem::{
        CreateDirectoryW, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE, READ_CONTROL,
    },
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};

/// Each pointer is either null or a unique LocalAlloc-family API output.
struct LocalMemory(*mut c_void);
#[expect(
    unsafe_code,
    reason = "Own and release Windows LocalAlloc-family security outputs"
)]
impl Drop for LocalMemory {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: The APIs below transfer these unique allocations to us.
            unsafe {
                LocalFree(self.0);
            }
        }
    }
}

fn last_error(operation: &'static str) -> nw_domain::DomainError {
    diagnostic_io_error(operation, std::io::Error::last_os_error())
}
fn status_error(operation: &'static str, status: u32) -> nw_domain::DomainError {
    diagnostic_io_error(operation, std::io::Error::from_raw_os_error(status as i32))
}
fn decode_error(
    operation: &'static str,
    error: std::string::FromUtf16Error,
) -> nw_domain::DomainError {
    diagnostic_io_error(
        operation,
        std::io::Error::new(std::io::ErrorKind::InvalidData, error),
    )
}

#[expect(
    unsafe_code,
    reason = "Read bounded TokenUser data and own the returned process token/SID text"
)]
fn current_sid() -> Result<String> {
    let mut token = ptr::null_mut();
    // SAFETY: Valid pseudo process handle and initialized output; no impersonation.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(last_error("acl.token_open"));
    }
    if token.is_null() {
        return Err(denied("Windows process token is unavailable"));
    }
    // SAFETY: OpenProcessToken succeeded and transferred this unique real handle.
    let token = unsafe { OwnedHandle::from_raw_handle(token) };
    let mut needed = 0;
    // SAFETY: The zero-buffer query writes only the required-size output.
    let query = unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            ptr::null_mut(),
            0,
            &mut needed,
        )
    };
    if query != 0
        || std::io::Error::last_os_error().raw_os_error() != Some(ERROR_INSUFFICIENT_BUFFER as i32)
        || !(std::mem::size_of::<TOKEN_USER>() as u32..=32768).contains(&needed)
    {
        return Err(denied("Windows TokenUser size could not be established"));
    }
    // usize storage supplies TOKEN_USER's required pointer alignment.
    let mut buffer = vec![0usize; (needed as usize).div_ceil(std::mem::size_of::<usize>())];
    // SAFETY: Aligned, initialized storage covers the exact requested byte length.
    if unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            buffer.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    } == 0
    {
        return Err(last_error("acl.token_read"));
    }
    // SAFETY: A successful TokenUser query initialized TOKEN_USER at this aligned address.
    let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
    let mut text = ptr::null_mut();
    // SAFETY: The SID belongs to the still-live TokenUser buffer; output is LocalAlloc-owned.
    if unsafe { ConvertSidToStringSidW(user.User.Sid, &mut text) } == 0 {
        return Err(last_error("acl.sid_text"));
    }
    let allocation = LocalMemory(text.cast());
    if allocation.0.is_null() {
        return Err(denied("Windows SID text is unavailable"));
    }
    // A valid Windows SID has at most 15 subauthorities and fits this text bound.
    for length in 0..256 {
        // SAFETY: ConvertSidToStringSidW returns valid NUL-terminated SID text.
        if unsafe { *text.add(length) } == 0 {
            // SAFETY: This is the initialized prefix preceding its terminator.
            let units = unsafe { std::slice::from_raw_parts(text, length) };
            return String::from_utf16(units)
                .map_err(|error| decode_error("acl.sid_decode", error));
        }
    }
    Err(denied("Windows SID text exceeds its bound"))
}

fn open_object(path: &Path) -> Result<File> {
    let file = OpenOptions::new()
        .access_mode(READ_CONTROL | FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|error| diagnostic_io_error("acl.object_open", error))?;
    let metadata = file
        .metadata()
        .map_err(|error| diagnostic_io_error("acl.object_metadata", error))?;
    if (!metadata.is_dir() && !metadata.is_file())
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    {
        return Err(denied(
            "Protected ACL object must be an ordinary file or directory",
        ));
    }
    Ok(file)
}

#[expect(
    unsafe_code,
    reason = "Build an owned descriptor from the actual current process SID"
)]
fn new_directory_descriptor(sid: &str) -> Result<LocalMemory> {
    let text = format!("D:P(A;OICI;FA;;;{sid})")
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let mut descriptor = ptr::null_mut();
    // SAFETY: Valid NUL-terminated trusted SID descriptor and initialized output.
    let okay = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            text.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            ptr::null_mut(),
        )
    };
    if okay == 0 {
        return Err(last_error("acl.descriptor_create"));
    }
    let allocation = LocalMemory(descriptor);
    if allocation.0.is_null() {
        return Err(denied("Protected security descriptor is absent"));
    }
    Ok(allocation)
}

/// Supply the protected DACL at creation. Never change an existing object's ACL,
/// including one that appears after an absence observation.
pub(super) fn create_directory(path: &Path) -> Result<()> {
    let absolute = std::path::absolute(path)
        .map_err(|error| diagnostic_io_error("acl.create_absolute", error))?;
    let mut missing = Vec::new();
    for candidate in absolute.ancestors() {
        if candidate
            .try_exists()
            .map_err(|error| diagnostic_io_error("acl.create_exists", error))?
        {
            break;
        }
        missing.push(candidate);
    }
    let sid = current_sid()?;
    if missing.is_empty() {
        return verify_directory(&absolute, &sid);
    }
    let descriptor = new_directory_descriptor(&sid)?;
    // Borrowed ancestor slices avoid recursive stack growth or cloning every
    // path prefix. Protect each newly created missing component, not just leaf.
    for candidate in missing.into_iter().rev() {
        let parent = fs::canonicalize(
            candidate
                .parent()
                .ok_or_else(|| denied("Protected directory parent is absent"))?,
        )
        .map_err(|error| diagnostic_io_error("acl.create_parent", error))?;
        let name = candidate
            .file_name()
            .ok_or_else(|| denied("Protected directory name is absent"))?;
        // canonicalize supplies Windows' extended absolute parent path, including
        // long-path/UNC support. The leaf is never resolved as creation authority.
        let target = parent.join(name);
        create_component(&target, &descriptor)?;
        // ERROR_ALREADY_EXISTS supplies no ownership/creation authority. This
        // path only reads the actual DACL and refuses a raced-in broad object.
        verify_directory(&target, &sid)?;
    }
    verify_directory(&absolute, &sid)
}

/// Returns whether this call created the component. Existing objects are never
/// modified; the caller must verify them independently before proceeding.
#[expect(
    unsafe_code,
    reason = "Create a directory with a live trusted security descriptor"
)]
fn create_component(path: &Path, descriptor: &LocalMemory) -> Result<bool> {
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: 0,
    };
    let mut wide = path.as_os_str().encode_wide().collect::<Vec<_>>();
    if wide.contains(&0) || wide.len() >= 32767 {
        return Err(denied(
            "Protected directory path exceeds the native path bound",
        ));
    }
    wide.push(0);
    // SAFETY: Valid NUL-terminated path and initialized attributes whose borrowed
    // descriptor stays live. Windows copies security on successful creation.
    if unsafe { CreateDirectoryW(wide.as_ptr(), &attributes) } != 0 {
        return Ok(true);
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(ERROR_ALREADY_EXISTS as i32) {
        return Ok(false);
    }
    Err(diagnostic_io_error("acl.directory_create", error))
}

/// Invoke the actual creation collision even though the fixture already knows
/// the directory exists. The outer result proves the native collision occurred;
/// the inner result is the same actual-DACL verification used in production.
#[cfg(test)]
pub(super) fn creation_collision(path: &Path) -> Result<Result<()>> {
    let sid = current_sid()?;
    let descriptor = new_directory_descriptor(&sid)?;
    if create_component(path, &descriptor)? {
        return Err(denied(
            "Native collision fixture unexpectedly created a directory",
        ));
    }
    Ok(verify_directory(path, &sid))
}

#[expect(
    unsafe_code,
    reason = "Inspect the held object's actual DACL and copy its bounded native SDDL"
)]
fn descriptor_text(file: &File) -> Result<String> {
    let mut descriptor = ptr::null_mut();
    // SAFETY: The held handle has READ_CONTROL; all unused outputs are null.
    let status = unsafe {
        GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            &mut descriptor,
        )
    };
    if status != 0 {
        return Err(status_error("acl.dacl_read", status));
    }
    let allocation = LocalMemory(descriptor);
    if allocation.0.is_null() {
        return Err(denied("Protected security descriptor is absent"));
    }
    let mut text = ptr::null_mut();
    let mut length = 0;
    // SAFETY: The descriptor remains live and outputs receive a unique LocalAlloc string.
    let okay = unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            allocation.0,
            SDDL_REVISION_1,
            DACL_SECURITY_INFORMATION,
            &mut text,
            &mut length,
        )
    };
    if okay == 0 {
        return Err(last_error("acl.dacl_text"));
    }
    let text_allocation = LocalMemory(text.cast());
    if text_allocation.0.is_null() || !(1..=256 * 1024).contains(&length) {
        return Err(denied("Windows ACL text exceeds its bound"));
    }
    // SAFETY: The API reports its initialized UTF-16 length including the terminator.
    let units = unsafe { std::slice::from_raw_parts(text, length as usize) };
    if units.last() != Some(&0) {
        return Err(denied("Windows ACL text is unterminated"));
    }
    String::from_utf16(&units[..units.len() - 1])
        .map_err(|error| decode_error("acl.dacl_decode", error))
}

fn verify(path: &Path, sid: &str) -> Result<File> {
    let file = open_object(path)?;
    check_dacl(&descriptor_text(&file)?, sid)?;
    // The checked handle must still be the object named by the caller's path.
    let current = open_object(path)?;
    if native_object(&file)? != native_object(&current)? {
        return Err(denied("Protected ACL object changed during verification"));
    }
    Ok(file)
}

fn verify_directory(path: &Path, sid: &str) -> Result<()> {
    let file = verify(path, sid)?;
    if !file
        .metadata()
        .map_err(|error| diagnostic_io_error("acl.directory_metadata", error))?
        .is_dir()
    {
        return Err(denied("Protected directory must be an ordinary directory"));
    }
    Ok(())
}

pub(super) fn protect(path: &Path) -> Result<()> {
    verify(path, &current_sid()?).map(|_| ())
}
