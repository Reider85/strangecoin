//! Memory-pointer host ABI (S1.5-P10).
//!
//! Byte buffers cross the WASM/host boundary as `(ptr: i32, len: i32)`
//! pairs: the contract keeps data in its exported `memory` and passes
//! offsets; hosts read inputs with [`read_bytes`] / [`read_array32`] and
//! write outputs with [`write_bytes`]. Every access is bounds-checked
//! against the current linear-memory size (threat V-21: a malicious module
//! must not be able to make the host panic or read out of bounds); a bad
//! pointer aborts the call frame with a host error rather than trapping the
//! whole engine.

use wasmi::{Caller, Extern, Memory};

/// Resolve the module's exported `memory`.
pub(crate) fn get_memory<T>(caller: &Caller<'_, T>) -> Result<Memory, wasmi::Error> {
    caller
        .get_export("memory")
        .and_then(Extern::into_memory)
        .ok_or_else(|| wasmi::Error::new("host ABI: module does not export `memory`"))
}

fn checked_range(ptr: u32, len: usize, mem_size: usize) -> Result<(usize, usize), wasmi::Error> {
    let start = ptr as usize;
    let end = start
        .checked_add(len)
        .ok_or_else(|| wasmi::Error::new("host ABI: pointer + length overflow"))?;
    if end > mem_size {
        return Err(wasmi::Error::new("host ABI: access out of bounds"));
    }
    Ok((start, end))
}

/// Read `len` bytes from the caller's linear memory at `ptr`.
pub(crate) fn read_bytes<T>(
    caller: &Caller<'_, T>,
    ptr: u32,
    len: u32,
) -> Result<Vec<u8>, wasmi::Error> {
    let memory = get_memory(caller)?;
    let (start, end) = checked_range(ptr, len as usize, memory.data_size(caller))?;
    let mut buf = vec![0u8; end - start];
    memory
        .read(caller, start, &mut buf)
        .map_err(|e| wasmi::Error::new(format!("host ABI: memory read: {e}")))?;
    Ok(buf)
}

/// Read exactly 32 bytes at `ptr` (hash/key convention of the host surface).
pub(crate) fn read_array32<T>(
    caller: &Caller<'_, T>,
    ptr: u32,
) -> Result<[u8; 32], wasmi::Error> {
    let bytes = read_bytes(caller, ptr, 32)?;
    let arr: [u8; 32] = bytes
        .try_into()
        .expect("read_bytes returns exactly 32 bytes");
    Ok(arr)
}

/// Write `data` into the caller's linear memory at `ptr`.
pub(crate) fn write_bytes<T>(
    caller: &mut Caller<'_, T>,
    ptr: u32,
    data: &[u8],
) -> Result<(), wasmi::Error> {
    let memory = get_memory(caller)?;
    let mem_size = memory.data_size(&*caller);
    let (start, _) = checked_range(ptr, data.len(), mem_size)?;
    memory
        .write(caller, start, data)
        .map_err(|e| wasmi::Error::new(format!("host ABI: memory write: {e}")))?;
    Ok(())
}
