//! Import from a local Conductor install without opening the app:
//! `cargo run --example import_conductor`
fn main() -> anyhow::Result<()> {
    let db = purrfectpr_lib::importer::default_db_path().ok_or_else(|| anyhow::anyhow!("no Conductor data found"))?;
    let path = dirs::data_dir().unwrap().join("dev.purrfectpr.app").join("purrfectpr.db");
    let store = purrfectpr_lib::store::Store::open(&path)?;
    println!("{:?}", purrfectpr_lib::importer::import(&store, &db)?);
    Ok(())
}
