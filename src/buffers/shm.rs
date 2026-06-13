use crate::buffers::CaptureBuffer;
use crate::error::{Result, WlsnipError};

use std::os::fd::{AsFd, AsRawFd, OwnedFd};
use nix::sys::memfd::{memfd_create, MFdFlags};
use nix::unistd::ftruncate;

/// Create an anonymous file descriptor backed by memory, suitable for Wayland SHM.
///
/// Uses `memfd_create` for a purely in-memory fd — no filesystem path needed.
pub fn create_shm_fd(size: usize) -> Result<OwnedFd> {
    let fd = memfd_create(
        c"wlsnip-shm",
        MFdFlags::MFD_CLOEXEC | MFdFlags::MFD_ALLOW_SEALING,
    )
    .map_err(|e| WlsnipError::BufferAlloc(format!("memfd_create failed: {e}")))?;

    ftruncate(&fd, size as i64)
        .map_err(|e| WlsnipError::BufferAlloc(format!("ftruncate failed: {e}")))?;

    Ok(fd)
}

/// A Wayland SHM memory pool wrapping an mmap'd file descriptor.
///
/// On drop, the mapping is automatically unmapped.
pub struct ShmPool {
    /// The file descriptor backing this pool
    fd: OwnedFd,
    /// Pointer to the mmap'd region
    ptr: *mut u8,
    /// Size of the mapped region in bytes
    size: usize,
}

// SAFETY: The ShmPool owns its fd and memory exclusively.
// The mmap'd region is only accessed through &self or &mut self,
// and we ensure no concurrent access via Rust's borrow checker.
unsafe impl Send for ShmPool {}

impl ShmPool {
    /// Create a new SHM pool with the given size.
    pub fn new(size: usize) -> Result<Self> {
        let fd = create_shm_fd(size)?;

        // SAFETY: We just created this fd and set its size. The mmap is valid
        // for the lifetime of the fd, and we unmap on Drop.
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd.as_fd().as_raw_fd(),
                0,
            )
        };

        if ptr == libc::MAP_FAILED {
            return Err(WlsnipError::BufferAlloc(format!(
                "mmap failed: {}",
                std::io::Error::last_os_error()
            )));
        }

        Ok(Self {
            fd,
            ptr: ptr as *mut u8,
            size,
        })
    }

    /// Get a mutable slice to the entire pool memory.
    #[allow(dead_code)]
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        // SAFETY: ptr is valid for `size` bytes, we own it exclusively via &mut self
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.size) }
    }

    /// Get an immutable slice to the entire pool memory.
    pub fn as_slice(&self) -> &[u8] {
        // SAFETY: ptr is valid for `size` bytes, no mutable alias exists
        unsafe { std::slice::from_raw_parts(self.ptr, self.size) }
    }

    /// Borrow the underlying file descriptor (needed by Wayland wl_shm).
    pub fn fd(&self) -> &OwnedFd {
        &self.fd
    }

    /// Size of the pool in bytes.
    #[allow(dead_code)]
    pub fn size(&self) -> usize {
        self.size
    }

    /// Copy the pool contents into a `CaptureBuffer`.
    pub fn to_capture_buffer(
        &self,
        width: u32,
        height: u32,
        stride: u32,
        format: crate::buffers::PixelFormat,
    ) -> CaptureBuffer {
        let data_size = (stride * height) as usize;
        let data = self.as_slice()[..data_size].to_vec();
        CaptureBuffer {
            width,
            height,
            stride,
            format,
            data,
        }
    }
}

impl Drop for ShmPool {
    fn drop(&mut self) {
        // SAFETY: We mapped this region in `new`, and we're the sole owner.
        unsafe {
            libc::munmap(self.ptr as *mut libc::c_void, self.size);
        }
    }
}
