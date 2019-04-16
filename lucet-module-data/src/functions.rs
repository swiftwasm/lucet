#[repr(C)]
#[derive(Clone, Debug)]
pub struct FunctionSpec {
    addr: u64,
    len: u64,
}

impl FunctionSpec {
    pub fn new(addr: u64, len: u64) -> Self {
        FunctionSpec { addr, len }
    }
    pub fn contains(&self, addr: u64) -> bool {
        // TODO This *may* be an off by one - replicating the check in
        // looking up trap manifest addresses. Need to verify if the
        // length produced by Cranelift is of an inclusive or exclusive range
        addr >= self.addr && addr <= (self.addr + self.len)
    }
}
