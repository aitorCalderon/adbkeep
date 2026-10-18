//! Lists and verifies the backups in a folder.
//! `cargo run -p adbkeep-core --example library -- <dir>`

use adbkeep_core::library;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = std::env::args().nth(1).ok_or("usage: library <dir>")?;
    for b in library::list(std::path::Path::new(&dir))? {
        println!("{} packed={} {:?} {} bytes", b.name, b.packed, b.status, b.size);
        if b.manifest.is_some() {
            let start = std::time::Instant::now();
            let r = library::verify(std::path::Path::new(&b.path), |_, _| {})?;
            println!("  verify: {} files, problems {:?} ({:?})", r.files_checked, r.problems, start.elapsed());
        }
    }
    Ok(())
}
