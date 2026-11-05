//! Import from a local Conductor install without opening the app:
//! `cargo run --example import_conductor`
fn main() -> anyhow::Result<()> {
    let db = nekotrail_lib::importer::default_db_path().ok_or_else(|| anyhow::anyhow!("no Conductor data found"))?;
    let path = dirs::data_dir().unwrap().join("dev.nekotrail.app").join("nekotrail.db");
    let store = nekotrail_lib::store::Store::open(&path)?;
    println!("{:?}", nekotrail_lib::importer::import(&store, &db)?);
    Ok(())
}
