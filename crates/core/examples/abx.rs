//! Converts ABX files to text XML and counts their elements.
//! `cargo run -p adbkeep-core --example abx -- <file>...`

fn count(e: &adbkeep_core::abx::Element) -> usize {
    1 + e.elements().map(count).sum::<usize>()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    for path in std::env::args().skip(1) {
        let all = adbkeep_core::abx::parse_all(&std::fs::read(&path)?)?;
        std::fs::write(format!("{path}.ours.xml"), adbkeep_core::abx::to_xml(&all))?;
        println!("{path}: root <{}>, elements={}", all[0].name, all.iter().map(count).sum::<usize>());
    }
    Ok(())
}
