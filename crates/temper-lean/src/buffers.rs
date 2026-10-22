//! Typed binary calls with RAII result ownership and scoped zero-copy views.
use anyhow::{Result, ensure};
use std::ffi::c_void;

#[repr(C)]
struct Input {
    data: *const u8,
    len: usize,
}

type Call = unsafe extern "C" fn(u64, *const Input, usize) -> *mut c_void;
type Status = unsafe extern "C" fn(*const c_void) -> i32;
type Count = unsafe extern "C" fn(*const c_void) -> usize;
type View = unsafe extern "C" fn(*const c_void, usize, *mut usize) -> *const u8;
type Free = unsafe extern "C" fn(*mut c_void);

pub(crate) struct Functions {
    call: Call,
    status: Status,
    count: Count,
    view: View,
    free: Free,
}

struct Reply {
    ptr: *mut c_void,
    free: Free,
}
impl Drop for Reply {
    fn drop(&mut self) {
        unsafe { (self.free)(self.ptr) };
    }
}

impl Functions {
    /// The library is built from the pinned checked entry and trusted bridge.
    pub(crate) unsafe fn load(library: &libloading::Library) -> Result<Self> {
        unsafe {
            Ok(Self {
                call: *library.get(b"temper_buffers_call\0")?,
                status: *library.get(b"temper_buffers_status\0")?,
                count: *library.get(b"temper_buffers_count\0")?,
                view: *library.get(b"temper_buffers_view\0")?,
                free: *library.get(b"temper_buffers_free\0")?,
            })
        }
    }

    /// Caller holds the native-core mutex. No Lean references escape this scope.
    pub(crate) fn with<T>(
        &self,
        parameter: u64,
        inputs: &[&[u8]],
        callback: impl FnOnce(std::result::Result<&[&[u8]], &str>) -> T,
    ) -> Result<T> {
        let inputs: Vec<_> = inputs
            .iter()
            .map(|b| Input {
                data: b.as_ptr(),
                len: b.len(),
            })
            .collect();
        let ptr = unsafe { (self.call)(parameter, inputs.as_ptr(), inputs.len()) };
        ensure!(!ptr.is_null(), "Lean binary call failed");
        let reply = Reply {
            ptr,
            free: self.free,
        };
        let view = |index| -> Result<&[u8]> {
            let mut size = 0;
            let data = unsafe { (self.view)(reply.ptr, index, &mut size) };
            ensure!(!data.is_null(), "invalid Lean buffer view");
            // SAFETY: live, proved ByteArray/String, owned by reply on this thread.
            Ok(unsafe { std::slice::from_raw_parts(data, size) })
        };
        if unsafe { (self.status)(reply.ptr) } == 1 {
            return Ok(callback(Err(std::str::from_utf8(view(0)?)?)));
        }
        let outputs = (0..unsafe { (self.count)(reply.ptr) })
            .map(view)
            .collect::<Result<Vec<_>>>()?;
        Ok(callback(Ok(&outputs)))
    }
}
