//! Trusted binary ABI adapter. Objects never cross the host allocator boundary.
use lean_ffi::{
    include,
    object::{LeanArray, LeanByteArray, LeanExcept, LeanOwned},
};

/// A borrowed input slice. The bridge copies it once into a Lean ByteArray.
#[repr(C)]
pub struct Bytes {
    data: *const u8,
    len: usize,
}

unsafe extern "C" {
    fn temper_lean_invoke(
        parameter: u64,
        files: *mut include::lean_object,
    ) -> *mut include::lean_object;
    fn lean_initialize_thread();
    fn lean_finalize_thread();
}

struct Thread;
impl Drop for Thread {
    fn drop(&mut self) {
        unsafe { lean_finalize_thread() };
    }
}

/// Field drop order releases all Lean references before finalizing the thread.
pub struct Reply {
    value: LeanExcept<LeanOwned>,
    _thread: Thread,
}

/// Execute on the calling thread. The returned handle must be freed on that thread.
///
/// # Safety
/// Input slices must remain readable for the call; Lean must be initialized.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn temper_rs_buffers_call(
    parameter: u64,
    inputs: *const Bytes,
    count: usize,
) -> *mut Reply {
    unsafe { lean_initialize_thread() };
    let thread = Thread;
    let inputs = if count == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(inputs, count) }
    };
    let mut files = LeanArray::alloc(0);
    for input in inputs {
        let bytes = if input.len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(input.data, input.len) }
        };
        files = files.push(LeanByteArray::from_bytes(bytes));
    }
    // Ownership transfers to the proved function and back into the RAII result.
    let value = unsafe { LeanExcept::from_raw(temper_lean_invoke(parameter, files.into_raw())) };
    Box::into_raw(Box::new(Reply {
        value,
        _thread: thread,
    }))
}

/// Return 0 for success and 1 for a Lean error.
/// # Safety
/// The handle must be live on this thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn temper_rs_buffers_status(reply: *const Reply) -> i32 {
    i32::from(unsafe { &*reply }.value.is_error())
}

/// Count successful output buffers.
/// # Safety
/// The handle must be live on this thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn temper_rs_buffers_count(reply: *const Reply) -> usize {
    match unsafe { &*reply }.value.into_result() {
        Ok(value) => value.as_array().len(),
        Err(_) => 0,
    }
}

/// Borrow an output buffer, or the error text when status is 1. No copy.
/// # Safety
/// The handle and length pointer must be live; the view expires at free.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn temper_rs_buffers_view(
    reply: *const Reply,
    index: usize,
    len: *mut usize,
) -> *const u8 {
    let value = unsafe { &*reply }.value.into_result();
    match value {
        Ok(value) => {
            let array = value.as_array();
            if index >= array.len() {
                unsafe { *len = 0 };
                return std::ptr::null();
            }
            let buffer = array.get(index).as_byte_array();
            let bytes = buffer.as_bytes();
            unsafe { *len = bytes.len() };
            bytes.as_ptr()
        }
        Err(value) => {
            let error = value.as_string();
            let bytes = error.as_str().as_bytes();
            unsafe { *len = bytes.len() };
            bytes.as_ptr()
        }
    }
}

/// Release the result before finalizing the calling thread's Lean context.
/// # Safety
/// Free exactly once, on the thread which called buffers_call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn temper_rs_buffers_free(reply: *mut Reply) {
    drop(unsafe { Box::from_raw(reply) });
}
