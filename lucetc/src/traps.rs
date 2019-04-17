use cranelift_codegen::ir;
use cranelift_faerie::traps::FaerieTrapManifest;

use byteorder::{LittleEndian, WriteBytesExt};
use faerie::{Artifact, Decl, Link};
use failure::{Error, ResultExt};
use lucet_module_data::{TrapManifestRecord, TrapSite};
use std::io::Cursor;

pub fn write_trap_manifest(manifest: &FaerieTrapManifest, obj: &mut Artifact) -> Result<(), Error> {
    // declare traptable symbol
    let manifest_len_sym = "lucet_trap_manifest_len";
    obj.declare(&manifest_len_sym, Decl::data().global())
        .context(format!("declaring {}", &manifest_len_sym))?;

    let manifest_sym = "lucet_trap_manifest";
    obj.declare(&manifest_sym, Decl::data().global())
        .context(format!("declaring {}", &manifest_sym))?;

    let manifest_row_size = std::mem::size_of::<TrapManifestRecord>();

    let manifest_len = manifest.sinks.len();
    let mut manifest_len_buf: Vec<u8> = Vec::new();
    manifest_len_buf
        .write_u32::<LittleEndian>(manifest_len as u32)
        .unwrap();
    obj.define(&manifest_len_sym, manifest_len_buf)
        .context(format!("defining {}", &manifest_len_sym))?;

    let mut trap_manifest: Vec<TrapManifestRecord> = vec![];

    for (i, sink) in manifest.sinks.iter().enumerate() {
        trap_manifest.push(TrapManifestRecord {
            table_addr: 0, // This will have a relocation
            table_len: sink.sites.len() as u64,
            func_index: i as u32,
        });

        let func_sym = &sink.name;
        let trap_sym = trap_sym_for_func(func_sym);

        obj.declare(&trap_sym, Decl::data().global())
            .context(format!("declaring {}", &trap_sym))?;

        // table for this function is provided via a link (abs8 relocation)
        obj.link(Link {
            from: &manifest_sym,
            to: &trap_sym,
            at: (i * std::mem::size_of::<TrapManifestRecord>()) as u64,
        })
        .context("linking trap table into trap manifest")?;

        // ok, now write the actual function-level trap table
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

    let trap_manifest_bytes = unsafe {
        std::slice::from_raw_parts(
            trap_manifest.as_ptr() as *const u8,
            trap_manifest.len() * std::mem::size_of::<TrapManifestRecord>(),
        )
    };

    obj.define(&manifest_sym, trap_manifest_bytes.to_vec())
        .context(format!("defining {}", &manifest_sym))?;

    // iterate over tables:
    //   write empty relocation thunk
    //   link from traptable symbol + thunk offset to function symbol
    //   write trapsite count
    //
    //   iterate over trapsites:
    //     write offset
    //     write trapcode

    Ok(())
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
