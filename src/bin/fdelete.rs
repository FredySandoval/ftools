#[allow(dead_code)]
#[path = "../fileclip_ops.rs"]
mod fileclip_ops;

use std::process;

fn main() {
    if let Err(message) = fileclip_ops::fdelete_main() {
        eprintln!("{message}");
        process::exit(1);
    }
}
