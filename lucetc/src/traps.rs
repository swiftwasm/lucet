use cranelift_codegen::ir;
use cranelift_faerie::traps::FaerieTrapManifest;
use cranelift_faerie::FaerieBackend;
use cranelift_module::ModuleFunction;

use faerie::{Artifact, Decl};
use failure::{Error, ResultExt};
use lucet_module_data::{FunctionSpec, TrapManifestRecord, TrapSite};

pub fn write_trap_tables(
    manifest: &FaerieTrapManifest,
    functions: &Vec<(String, FunctionSpec)>,
    obj: &mut Artifact,
) -> Result<Vec<TrapManifestRecord>, Error> {
    let mut trap_manifest: Vec<TrapManifestRecord> = Vec::with_capacity(manifest.sinks.len());

    for sink in manifest.sinks.iter() {
        let func_index: usize = {
            let mut trap_fn_index: Option<usize> = None;
            for (i, (func_name, func_spec)) in functions.iter().enumerate() {
                if func_name == &sink.name {
                    trap_fn_index = Some(i);
                    break;
                }
            }
            trap_fn_index.unwrap_or_else(|| {
                panic!(
                    "Trap sink {} has no corresponding function in the function manifest. This should not be possible?",
                    sink.name
                )
            })
        };

        trap_manifest.push(TrapManifestRecord {
            table_addr: 0, // This will be fixed up when loaded
            table_len: sink.sites.len() as u64,
            func_index: func_index as u32,
        });

        let func_sym = &sink.name;
        let trap_sym = trap_sym_for_func(func_sym);

        obj.declare(&trap_sym, Decl::data().global())
            .context(format!("declaring {}", &trap_sym))?;

        // write the actual function-level trap table
        let traps: Vec<TrapSite> = sink
            .sites
            .iter()
            .map(|site| TrapSite {
                offset: site.offset,
                code: translate_trapcode(site.code),
            })
            .collect();

        let trap_site_bytes = unsafe {
            std::slice::from_raw_parts(
                traps.as_ptr() as *const u8,
                traps.len() * std::mem::size_of::<TrapSite>(),
            )
        };

        // and write the function trap table into the object
        obj.define(&trap_sym, trap_site_bytes.to_vec())
            .context(format!("defining {}", &trap_sym))?;
    }

    Ok(trap_manifest)
}

fn trap_sym_for_func(sym: &str) -> String {
    return format!("lucet_trap_table_{}", sym);
}

// Trapcodes can be thought of as a tuple of (type, subtype). Each are
// represented as a 16-bit unsigned integer. These are packed into a u32
// wherein the type occupies the low 16 bites and the subtype takes the
// high bits.
//
// Not all types have subtypes. Currently, only the user User type has a
// subtype.
fn translate_trapcode(code: ir::TrapCode) -> lucet_module_data::TrapCode {
    match code {
        ir::TrapCode::StackOverflow => lucet_module_data::TrapCode::StackOverflow,
        ir::TrapCode::HeapOutOfBounds => lucet_module_data::TrapCode::HeapOutOfBounds,
        ir::TrapCode::OutOfBounds => lucet_module_data::TrapCode::OutOfBounds,
        ir::TrapCode::IndirectCallToNull => lucet_module_data::TrapCode::IndirectCallToNull,
        ir::TrapCode::BadSignature => lucet_module_data::TrapCode::BadSignature,
        ir::TrapCode::IntegerOverflow => lucet_module_data::TrapCode::IntegerOverflow,
        ir::TrapCode::IntegerDivisionByZero => lucet_module_data::TrapCode::IntegerDivByZero,
        ir::TrapCode::BadConversionToInteger => lucet_module_data::TrapCode::BadConversionToInteger,
        ir::TrapCode::Interrupt => lucet_module_data::TrapCode::Interrupt,
        ir::TrapCode::TableOutOfBounds => lucet_module_data::TrapCode::TableOutOfBounds,
        ir::TrapCode::UnreachableCodeReached => lucet_module_data::TrapCode::Unreachable,
        ir::TrapCode::User(_) => panic!("we should never emit a user trapcode"),
    }
}
