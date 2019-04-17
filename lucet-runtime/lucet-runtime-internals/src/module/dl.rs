use crate::error::Error;
use crate::module::{
    AddrDetails, GlobalSpec, HeapSpec, Module, ModuleInternal, TableElement, TrapManifestRecord,
};
use libc::c_void;
use libloading::{Library, Symbol};
use lucet_module_data::{CodeMetadata, FunctionSpec, ModuleData};
use std::ffi::CStr;
use std::mem;
use std::path::Path;
use std::slice;
use std::slice::from_raw_parts;
use std::sync::Arc;

/// A Lucet module backed by a dynamically-loaded shared object.
pub struct DlModule {
    lib: Library,

    /// Base address of the dynamically-loaded module
    fbase: *const c_void,

    /// Metadata decoded from inside the module
    module_data: ModuleData<'static>,

    code_metadata: CodeMetadata,

    function_manifest: &'static [FunctionSpec],
}

// for the one raw pointer only
unsafe impl Send for DlModule {}
unsafe impl Sync for DlModule {}

impl DlModule {
    /// Create a module, loading code from a shared object on the filesystem.
    pub fn load<P: AsRef<Path>>(so_path: P) -> Result<Arc<Self>, Error> {
        // Load the dynamic library. The undefined symbols corresponding to the lucet_syscall_
        // functions will be provided by the current executable.  We trust our wasm->dylib compiler
        // to make sure these function calls are the way the dylib can touch memory outside of its
        // stack and heap.
        let abs_so_path = so_path.as_ref().canonicalize().map_err(Error::DlError)?;
        let lib = Library::new(abs_so_path.as_os_str()).map_err(Error::DlError)?;

        let module_data_ptr = unsafe {
            lib.get::<*const u8>(b"lucet_module_data").map_err(|e| {
                lucet_incorrect_module!("error loading required symbol `lucet_module_data`: {}", e)
            })?
        };

        let module_data_len = unsafe {
            lib.get::<usize>(b"lucet_module_data_len").map_err(|e| {
                lucet_incorrect_module!(
                    "error loading required symbol `lucet_module_data_len`: {}",
                    e
                )
            })?
        };

        // Deserialize the slice into ModuleData, which will hold refs into the loaded
        // shared object file in `module_data_slice`. Both of these get a 'static lifetime because
        // Rust doesn't have a safe way to describe that their lifetime matches the containing
        // struct (and the dll).
        //
        // The exposed lifetime of ModuleData will be the same as the lifetime of the
        // dynamically loaded library. This makes the interface safe.
        let module_data_slice: &'static [u8] =
            unsafe { slice::from_raw_parts(*module_data_ptr, *module_data_len) };
        let module_data = ModuleData::deserialize(module_data_slice)?;

        let fbase = if let Some(dli) = dladdr(*module_data_ptr as *const c_void) {
            dli.dli_fbase
        } else {
            std::ptr::null()
        };

        let code_metadata_ptr = unsafe {
            lib.get::<*const u8>(b"lucet_code_metadata").map_err(|e| {
                lucet_incorrect_module!(
                    "error loading required symbol `lucet_code_metadata`: {}",
                    e
                )
            })?
        };

        let code_metadata_len = unsafe {
            lib.get::<usize>(b"lucet_code_metadata_len").map_err(|e| {
                lucet_incorrect_module!(
                    "error loading required symbol `lucet_code_metadata_len`: {}",
                    e
                )
            })?
        };

        let code_metadata_slice: &'static [u8] =
            unsafe { slice::from_raw_parts(*code_metadata_ptr, *code_metadata_len) };
        let mut code_metadata = CodeMetadata::deserialize(code_metadata_slice)?;

        let function_manifest = unsafe {
            let manifest_len_ptr = lib.get::<*const u32>(b"lucet_function_manifest_len");
            let manifest_ptr = lib.get::<*const FunctionSpec>(b"lucet_function_manifest");

            match (manifest_ptr, manifest_len_ptr) {
                (Ok(ptr), Ok(len_ptr)) => {
                    let manifest_len = len_ptr.as_ref().ok_or(lucet_incorrect_module!(
                        "`lucet_function_manifest_len` is defined but null"
                    ))?;
                    let manifest = ptr.as_ref().ok_or(lucet_incorrect_module!(
                        "`lucet_function_manifest` is defined but null"
                    ))?;

                    from_raw_parts(manifest, *manifest_len as usize)
                }
                // TODO: Can a module be data-only?
                (Err(_), Err(_)) => &[],
                (Ok(_), Err(e)) => {
                    return Err(lucet_incorrect_module!(
                        "error loading symbol `lucet_function_manifest_len`: {}",
                        e
                    ));
                }
                (Err(e), Ok(_)) => {
                    return Err(lucet_incorrect_module!(
                        "error loading symbol `lucet_function_manifest`: {}",
                        e
                    ));
                }
            }
        };

        // Now that we have a function manifest, we can fix up table_addr values in
        // the trap manifest, which should point to the table in a readonly section

        for (idx, record) in code_metadata.trap_manifest.iter_mut().enumerate() {
            let function = function_manifest.get(record.func_index as usize).ok_or(
                lucet_incorrect_module!(
                    "trap manifest includes an invalid function index: {}",
                    record.func_index,
                ),
            )?;
            let fn_name = unsafe {
                let name_ptr = dladdr(function.addr as *const c_void)
                    .ok_or(lucet_incorrect_module!(
                        "no symbol for function at {:#016x}",
                        function.addr,
                    ))?
                    .dli_sname;

                if name_ptr.is_null() {
                    return Err(lucet_incorrect_module!(
                        "symbol name for {:#016x} is null",
                        function.addr,
                    ));
                } else {
                    CStr::from_ptr(name_ptr).to_owned().into_string()?
                }
            };
            let trap_sym = format!("lucet_trap_table_{}", fn_name);
            let trap_manifest_ptr = unsafe {
                lib.get::<u64>(trap_sym.as_bytes()).map_err(|e| {
                    lucet_incorrect_module!(
                        "trap manifest exists for function `{}` but there was an error loading the trap manifest: {}",
                        trap_sym,
                        e,
                    )
                })?
            };
            record.table_addr = *trap_manifest_ptr;
        }

        Ok(Arc::new(DlModule {
            lib,
            fbase,
            module_data,
            code_metadata,
            function_manifest,
        }))
    }
}

impl Module for DlModule {}

impl ModuleInternal for DlModule {
    fn heap_spec(&self) -> Option<&HeapSpec> {
        self.module_data.heap_spec()
    }

    fn globals(&self) -> &[GlobalSpec] {
        self.module_data.globals_spec()
    }

    fn get_sparse_page_data(&self, page: usize) -> Option<&[u8]> {
        if let Some(ref sparse_data) = self.module_data.sparse_data() {
            *sparse_data.get_page(page)
        } else {
            None
        }
    }

    fn sparse_page_data_len(&self) -> usize {
        self.module_data.sparse_data().map(|d| d.len()).unwrap_or(0)
    }

    fn table_elements(&self) -> Result<&[TableElement], Error> {
        let p_table_segment: Symbol<*const TableElement> = unsafe {
            self.lib.get(b"guest_table_0").map_err(|e| {
                lucet_incorrect_module!("error loading required symbol `guest_table_0`: {}", e)
            })?
        };
        let p_table_segment_len: Symbol<*const usize> = unsafe {
            self.lib.get(b"guest_table_0_len").map_err(|e| {
                lucet_incorrect_module!("error loading required symbol `guest_table_0_len`: {}", e)
            })?
        };
        let len = unsafe { **p_table_segment_len };
        let elem_size = mem::size_of::<TableElement>();
        if len > std::u32::MAX as usize {
            lucet_incorrect_module!("table segment too long: {}", len);
        }
        Ok(unsafe { from_raw_parts(*p_table_segment, **p_table_segment_len as usize) })
    }

    fn get_export_func(&self, sym: &[u8]) -> Result<*const extern "C" fn(), Error> {
        let mut guest_sym: Vec<u8> = b"guest_func_".to_vec();
        guest_sym.extend_from_slice(sym);
        match unsafe { self.lib.get::<*const extern "C" fn()>(&guest_sym) } {
            Err(ref e) if is_undefined_symbol(e) => Err(Error::SymbolNotFound(
                String::from_utf8_lossy(sym).into_owned(),
            )),
            Err(e) => Err(Error::DlError(e)),
            Ok(f) => Ok(*f),
        }
    }

    fn get_func_from_idx(
        &self,
        table_id: u32,
        func_id: u32,
    ) -> Result<*const extern "C" fn(), Error> {
        if table_id != 0 {
            return Err(Error::FuncNotFound(table_id, func_id));
        }
        let table = self.table_elements()?;
        let func: extern "C" fn() = table
            .get(func_id as usize)
            .map(|element| unsafe { std::mem::transmute(element.rf) })
            .ok_or(Error::FuncNotFound(table_id, func_id))?;
        Ok(&func as *const extern "C" fn())
    }

    fn get_start_func(&self) -> Result<Option<*const extern "C" fn()>, Error> {
        // `guest_start` is a pointer to the function the module designates as the start function,
        // since we can't have multiple symbols pointing to the same function and guest code might
        // call it in the normal course of execution
        if let Ok(start_func) = unsafe {
            self.lib
                .get::<*const *const extern "C" fn()>(b"guest_start")
        } {
            if start_func.is_null() {
                lucet_incorrect_module!("`guest_start` is defined but null");
            }
            Ok(Some(unsafe { **start_func }))
        } else {
            Ok(None)
        }
    }

    fn trap_manifest(&self) -> &[TrapManifestRecord] {
        &self.code_metadata.trap_manifest
    }

    fn function_manifest(&self) -> &[FunctionSpec] {
        self.function_manifest
    }

    fn addr_details(&self, addr: *const c_void) -> Result<Option<AddrDetails>, Error> {
        if let Some(dli) = dladdr(addr) {
            let file_name = if dli.dli_fname.is_null() {
                None
            } else {
                Some(unsafe { CStr::from_ptr(dli.dli_fname).to_owned().into_string()? })
            };
            let sym_name = if dli.dli_sname.is_null() {
                None
            } else {
                Some(unsafe { CStr::from_ptr(dli.dli_sname).to_owned().into_string()? })
            };
            Ok(Some(AddrDetails {
                in_module_code: dli.dli_fbase as *const c_void == self.fbase,
                file_name,
                sym_name,
            }))
        } else {
            Ok(None)
        }
    }
}

fn is_undefined_symbol(e: &std::io::Error) -> bool {
    // gross, but I'm not sure how else to differentiate this type of error from other
    // IO errors
    format!("{}", e).contains("undefined symbol")
}

// TODO: PR to nix or libloading?
// TODO: possibly not safe to use without grabbing the mutex within libloading::Library?
fn dladdr(addr: *const c_void) -> Option<libc::Dl_info> {
    let mut info = unsafe { mem::uninitialized::<libc::Dl_info>() };
    let res = unsafe { libc::dladdr(addr, &mut info as *mut libc::Dl_info) };
    if res != 0 {
        Some(info)
    } else {
        None
    }
}
