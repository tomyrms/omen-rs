//! Prints the OMEN Gaming Hub package/services state (read only).
fn main() {
    println!("{:#?}", omen_core::vendor::status());
}
