extern crate tar;

use std::fs::File;
use tar::Builder;

fn main() {
    let file = File::create("foo.tar").unwrap();
    let mut a = Builder::new(file);

    a.append_path("README.md").unwrap();
    a.append_file("lib.rs", &mut File::open("src/lib.rs").unwrap())
        .unwrap();
}
