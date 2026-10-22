//! An example of listing the file names of entries in an archive.
//!
//! Takes a tarball on stdin and prints out all of the entries inside.

extern crate tar;

use std::io::stdin;

use tar::Archive;

fn main() {
    let mut ar = Archive::new(stdin());
    for file in ar.entries().unwrap() {
        let f = file.unwrap();
        println!("{}", f.path().unwrap().display());
    }
}
