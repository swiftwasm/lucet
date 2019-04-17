#[repr(C)]
#[derive(Clone, Debug)]
pub struct FunctionSpec {
    addr: u64,
    len: u32,
}

impl FunctionSpec {
    pub fn new(addr: u64, len: u32) -> Self {
        FunctionSpec { addr, len }
    }
    pub fn contains(&self, addr: u64) -> bool {
        // TODO This *may* be an off by one - replicating the check in
        // looking up trap manifest addresses. Need to verify if the
        // length produced by Cranelift is of an inclusive or exclusive range
        addr >= self.addr && (addr - self.addr) <= (self.len as u64)
    }
    pub fn relative_addr(&self, addr: u64) -> Option<u32> {
        if let Some(offset) = addr.checked_sub(self.addr) {
            if offset < (self.len as u64) {
                // self.len is u32, so if the above check succeeded
                // offset must implicitly be <= u32::MAX - the following
                // conversion will not truncate bits
                return Some(offset as u32);
            }
        }

        None
    }
}
