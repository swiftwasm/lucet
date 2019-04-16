use num_derive::FromPrimitive;
use num_traits::FromPrimitive;
use serde::{Deserialize, Serialize};

use std::ffi::c_void;
use std::slice::from_raw_parts;

/// The type of a WebAssembly
/// [trap](http://webassembly.github.io/spec/core/intro/overview.html#trap).
#[repr(u32)]
#[derive(Copy, Clone, Debug, FromPrimitive, PartialEq)]
pub enum TrapCode {
    StackOverflow = 0,
    HeapOutOfBounds = 1,
    OutOfBounds = 2,
    IndirectCallToNull = 3,
    BadSignature = 4,
    IntegerOverflow = 5,
    IntegerDivByZero = 6,
    BadConversionToInteger = 7,
    Interrupt = 8,
    TableOutOfBounds = 9,
    Unreachable = 10,
}

impl TrapCode {
    pub fn try_from_u32(v: u32) -> Option<TrapCode> {
        Self::from_u32(v)
    }
}

/// Trap information for an address in a compiled function
///
/// To support zero-copy deserialization of trap tables, this
/// must be repr(C) [to avoid cases where Rust may change the
/// layout in some future version, mangling the interpretation
/// of an old TrapSite struct]
#[repr(C)]
pub struct TrapSite {
    pub offset: u32,
    pub code: TrapCode
}

/// Trap information for an address in a compiled function
#[repr(C)]
pub struct TrapTable<'a> {
    function: u32, // TODO: what type for function indices - u32? usize? u64?
    traps: &'a [TrapSite]
}

#[repr(C)]
#[derive(Clone, Debug)]
pub struct TrapManifestRecord {
    pub func_addr: u64,
    pub func_len: u64,
    pub table_addr: u64,
    pub table_len: u64,
}

impl TrapManifestRecord {
    pub fn contains_addr(&self, addr: *const c_void) -> bool {
        let addr = addr as u64;
        // TODO: is this correct? off-by-one error?
        addr >= self.func_addr && addr <= self.func_addr + self.func_len
    }

    pub fn trapsites(&self) -> &[TrapSite] {
        let table_addr = self.table_addr as *const TrapSite;
        assert!(!table_addr.is_null());
        unsafe { from_raw_parts(table_addr, self.table_len as usize) }
    }

    pub fn lookup_addr(&self, addr: *const c_void) -> Option<TrapCode> {
        if !self.contains_addr(addr) {
            return None;
        }

        // predicate to find the trapsite for the addr via binary search
        let f =
            |ts: &TrapSite| (self.func_addr as usize + ts.offset as usize).cmp(&(addr as usize));

        let trapsites = self.trapsites();
        if let Ok(i) = trapsites.binary_search_by(f) {
            Some(trapsites[i].code)
        } else {
            None
        }
    }
}
