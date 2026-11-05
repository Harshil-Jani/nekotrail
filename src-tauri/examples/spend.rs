//! Print spend from a nekotrail database (after migrations): `cargo run --example spend -- <db>`.
fn main() -> anyhow::Result<()> {
    let s = nekotrail_lib::store::Store::open(std::path::Path::new(&std::env::args().nth(1).unwrap()))?;
    let week: f64 = s.daily_costs(7)?.iter().map(|r| r.2).sum();
    let all: f64 = s.workspace_costs()?.iter().map(|r| r.1).sum();
    println!("7 days ${week:.2}, all workspaces ${all:.2}");
    Ok(())
}
