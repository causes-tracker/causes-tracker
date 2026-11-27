include!(concat!(env!("OUT_DIR"), "/version.rs"));

fn main() {
    println!("built by {RUSTC_VERSION}");
}
