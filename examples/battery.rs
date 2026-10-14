//! Prints what patina-shell reads of the battery:
//! `cargo run -p patina-shell --example battery`.

fn main() {
    match patina_shell::power::read() {
        Some(b) => println!("{b:?}\n{}", b.describe()),
        None => println!("no battery"),
    }
}
