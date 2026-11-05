//! Read-only: what Storage would offer to clean. `cargo run --example storage_scan -- <workspace dirs...>`
fn main() {
    let mut total = 0;
    for d in std::env::args().skip(1) {
        let c = nekotrail_lib::git::cleanable(std::path::Path::new(&d));
        let kb: u64 = c.iter().map(|x| x.1).sum();
        total += kb;
        if kb > 0 { println!("{:>8} MB  {}  {:?}", kb / 1024, d.rsplit('/').next().unwrap_or(&d), c.iter().map(|x| &x.0).collect::<Vec<_>>()); }
    }
    println!("total cleanable: {} MB", total / 1024);
}
