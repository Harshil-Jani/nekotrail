//! Dry-run a Conductor import against a copy of a nekotrail database: `cargo run --example preview_import -- <db>`.
use nekotrail_lib::{importer, store::Store};

fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).expect("path to a nekotrail.db copy");
    let cur = Store::open(std::path::Path::new(&path))?;
    let scratch = Store::in_memory()?;
    for r in cur.repos()? { scratch.insert_repo(&r)?; }
    for w in cur.workspaces(true)? { scratch.insert_workspace(&w)?; }
    let db = importer::default_db_path().expect("no Conductor data");
    println!("{:?}", importer::import(&scratch, &db)?);
    Ok(())
}
