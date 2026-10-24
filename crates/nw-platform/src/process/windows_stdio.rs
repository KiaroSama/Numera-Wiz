//! Detached owners must not retain their frontend's captured standard streams.
//! Rust1.99.1 otherwise inherits every inheritable calling-process handle.
use nw_domain::{DomainError, Result};
use windows_sys::Win32::{
    Foundation::{HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE, SetHandleInformation},
    System::Console::{GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE},
};
#[expect(
    unsafe_code,
    reason = "Borrowed current-process standard handles only; clear inheritance without closing or transferring any handle"
)]
pub fn prevent_stdio_inheritance() -> Result<()> {
    for kind in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        // SAFETY: GetStdHandle borrows the current process standard handle.
        // SetHandleInformation changes only its inheritance bit. We never close
        // it; the existing CLI/MCP readers and writers retain the same handle.
        let success = unsafe {
            let handle = GetStdHandle(kind);
            if handle.is_null() || handle == INVALID_HANDLE_VALUE {
                continue;
            }
            SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0)
        };
        if success == 0 {
            return Err(DomainError::new(
                "PROCESS_ISOLATION_UNAVAILABLE",
                format!(
                    "Cannot prevent detached owner stdio inheritance: {}",
                    std::io::Error::last_os_error()
                ),
            ));
        }
    }
    // Command still deliberately creates/duplicates its explicitly configured
    // null child streams; those handles are unrelated to the frontend pipes.
    Ok(())
}
