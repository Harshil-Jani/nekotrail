//! Time the hot paths against a copy of a real database: `cargo run --release --example perf -- <db>`.
use std::time::Instant;
fn main() -> anyhow::Result<()> {
    let s = nekotrail_lib::store::Store::open(std::path::Path::new(&std::env::args().nth(1).unwrap()))?;
    let t = Instant::now(); let ws = s.workspaces(false)?; println!("workspaces: {} in {:?}", ws.len(), t.elapsed());
    let t = Instant::now(); let c = s.workspace_costs()?; println!("workspace_costs: {} in {:?}", c.len(), t.elapsed());
    let t = Instant::now(); let d = s.daily_costs(7)?; println!("daily_costs: {} in {:?}", d.len(), t.elapsed());
    let mut biggest = (String::new(), 0usize);
    for w in &ws { for sess in s.sessions(&w.id)? { let n = s.events(&sess.id)?.len(); if n > biggest.1 { biggest = (sess.id.clone(), n); } } }
    let t = Instant::now(); let ev = s.events(&biggest.0)?; println!("biggest chat: {} events loaded in {:?} ({} KB JSON)", ev.len(), t.elapsed(), serde_json::to_string(&ev)?.len() / 1024);
    let t = Instant::now();
    for w in &ws { let _ = nekotrail_lib::git::changes(std::path::Path::new(&w.path), &w.target_branch); }
    println!("git changes for all {} workspaces (serial): {:?}", ws.len(), t.elapsed());
    Ok(())
}
