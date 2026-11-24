//! Usage: cargo run --release -p moge-formats --example inspect -- <file.rbxl|rbxlx|rbxm> [Full-API-Dump.json]
//! Imports a file and reports class counts, warnings and (with a dump) properties unknown to the API.
use moge_core::{ApiDump, Tree};
use std::collections::BTreeMap;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.first() else {
        eprintln!("Usage: inspect <file> [Full-API-Dump.json]");
        std::process::exit(2);
    };
    let data = std::fs::read(path).unwrap_or_else(|e| {
        eprintln!("Cannot read '{path}': {e}");
        std::process::exit(1);
    });
    let dump = args.get(1).map(|p| {
        let text = std::fs::read_to_string(p).unwrap_or_else(|e| {
            eprintln!("Cannot read '{p}': {e}");
            std::process::exit(1);
        });
        let t = std::time::Instant::now();
        let d = ApiDump::from_json(&text).unwrap_or_else(|e| {
            eprintln!("API Dump rejected: {e}");
            std::process::exit(1);
        });
        println!("API Dump: {} classes in {:?}", d.class_count(), t.elapsed());
        d
    });

    let mut tree = Tree::new();
    let t = std::time::Instant::now();
    let imp = match moge_formats::load(&data, &mut tree) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("Import failed: {e}");
            std::process::exit(1);
        }
    };
    println!("Imported in {:?}; {} top-level roots", t.elapsed(), imp.roots.len());

    let mut classes: BTreeMap<String, usize> = BTreeMap::new();
    let mut unknown: BTreeMap<String, usize> = BTreeMap::new();
    let mut total = 0;
    for &r in &imp.roots {
        for id in std::iter::once(r).chain(tree.descendants(r)) {
            let Some(n) = tree.node(id) else { continue };
            total += 1;
            *classes.entry(n.class.clone()).or_default() += 1;
            if let Some(d) = &dump {
                if d.class(&n.class).is_none() {
                    *unknown.entry(format!("class {}", n.class)).or_default() += 1;
                    continue;
                }
                for k in n.props.keys() {
                    if d.property(&n.class, k).is_none() {
                        *unknown.entry(format!("{}.{}", n.class, k)).or_default() += 1;
                    }
                }
            }
        }
    }
    println!("{total} instances");
    let mut top: Vec<_> = classes.into_iter().collect();
    top.sort_by(|a, b| b.1.cmp(&a.1));
    for (c, n) in top.iter().take(15) {
        println!("  {n:>8}  {c}");
    }
    println!("{} warnings", imp.warnings.len());
    for w in imp.warnings.iter().take(20) {
        println!("  warning: {w}");
    }
    if dump.is_some() {
        println!("{} (class.property) pairs unknown to the API Dump", unknown.len());
        for (k, n) in unknown.iter().take(30) {
            println!("  {n:>8}  {k}");
        }
    }
}
