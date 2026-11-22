//! Writes every line to stdout and to results/<name>.md.

use std::fs::{File, OpenOptions};
use std::io::Write;

pub struct Report {
    file: File,
}

impl Report {
    pub fn new(name: &str) -> Self {
        std::fs::create_dir_all("results").expect("create results/");
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(format!("results/{name}.md"))
            .expect("open results file");
        Report { file }
    }

    pub fn line(&mut self, s: &str) {
        println!("{s}");
        writeln!(self.file, "{s}").expect("write results");
    }
}
