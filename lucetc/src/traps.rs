use cranelift_codegen::ir;
use cranelift_faerie::traps::FaerieTrapManifest;

use byteorder::{LittleEndian, WriteBytesExt};
use faerie::{Artifact, Decl, Link};
use failure::{Error, ResultExt};
use std::io::Cursor;

pub fn write_trap_manifest(manifest: &FaerieTrapManifest, obj: &mut Artifact) -> Result<(), Error> {
    // declare traptable symbol
    let manifest_len_sym = "lucet_trap_manifest_len";
    obj.declare(&manifest_len_sym, Decl::data().global())
        .context(format!("declaring {}", &manifest_len_sym))?;

    let manifest_sym = "lucet_trap_manifest";
    obj.declare(&manifest_sym, Decl::data().global())
        .context(format!("declaring {}", &manifest_sym))?;

    let manifest_len = manifest.sinks.len();
    let mut manifest_len_buf: Vec<u8> = Vec::new();
    manifest_len_buf
        .write_u32::<LittleEndian>(manifest_len as u32)
        .unwrap();
    obj.define(&manifest_len_sym, manifest_len_buf)
        .context(format!("defining {}", &manifest_len_sym))?;

    // Manifests are serialized with the following struct elements in order:
    // { func_start: ptr, func_len: u64, traps: ptr, traps_len: u64 }
    let manifest_row_size = 8 * 4;
    let mut manifest_buf: Cursor<Vec<u8>> =
        Cursor::new(Vec::with_capacity(manifest_len * manifest_row_size));

    for sink in manifest.sinks.iter() {
        let func_sym = &sink.name;
        let trap_sym = trap_sym_for_func(func_sym);

        // declare function-level trap table
        obj.declare(&trap_sym, Decl::data().global())
            .context(format!("declaring {}", &trap_sym))?;

        // function symbol is provided via a link (abs8 relocation)
        obj.link(Link {
            from: &manifest_sym,
            to: func_sym,
            at: manifest_buf.position(),
        })
        .context("linking function sym into trap manifest")?;
        manifest_buf.write_u64::<LittleEndian>(0).unwrap();

        // write function length
        manifest_buf
            .write_u64::<LittleEndian>(sink.code_size as u64)
            .unwrap();

        // table for this function is provided via a link (abs8 relocation)
        obj.link(Link {
            from: &manifest_sym,
            to: &trap_sym,
            at: manifest_buf.position(),
        })
        .context("linking trap table into trap manifest")?;
        manifest_buf.write_u64::<LittleEndian>(0).unwrap();

        // finally, write the length of the trap table
        manifest_buf
            .write_u64::<LittleEndian>(sink.sites.len() as u64)
            .unwrap();

        // ok, now write the actual function-level trap table
        let mut traps: Vec<u8> = Vec::new();

        for site in sink.sites.iter() {
            // write offset into trap table
            traps.write_u32::<LittleEndian>(site.offset as u32).unwrap();
            // write serialized trap code into trap table
            traps
                .write_u32::<LittleEndian>(translate_trapcode(site.code) as u32)
                .unwrap();
        }

        // and finally write the function trap table into the object
        obj.define(&trap_sym, traps)
            .context(format!("defining {}", &trap_sym))?;
    }

    obj.define(&manifest_sym, manifest_buf.into_inner())
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
