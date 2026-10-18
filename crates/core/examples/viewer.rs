//! Reads the viewers of a backup on the PC and writes its exports into a folder (no phone).
//! `cargo run -p adbkeep-core --example viewer -- <backup> <out-dir> [--packed]`
//! With `--packed`, a folder backup is first packed into `<out-dir>/packed.adbkeep` and read from there.
//! Prints only counts, never message or contact contents.

use std::path::PathBuf;

use adbkeep_core::viewer;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let mut backup = PathBuf::from(args.get(1).ok_or("backup path")?);
    let out = PathBuf::from(args.get(2).ok_or("output dir")?);
    std::fs::create_dir_all(&out)?;
    if args.iter().any(|a| a == "--packed") {
        let packed = out.join("packed.adbkeep");
        adbkeep_core::archive::pack(&backup, &packed, None)?;
        backup = packed;
    }
    let start = std::time::Instant::now();
    let conv = viewer::messages(&backup)?;
    println!("conversations {} messages {}", conv.len(), conv.iter().map(|c| c.messages.len()).sum::<usize>());
    println!("contacts {}", viewer::contacts(&backup)?.len());
    println!("calls {}", viewer::calls(&backup)?.len());
    println!("wifi {}", viewer::wifi(&backup, false)?.len());
    println!("settings {}", viewer::settings(&backup)?.len());
    println!("sms.xml {}", viewer::export_sms_xml(&backup, &out.join("sms.xml"))?);
    println!("contacts.vcf {}", viewer::export_vcf(&backup, &out.join("contacts.vcf"))?);
    println!("took {:?}", start.elapsed());
    Ok(())
}
