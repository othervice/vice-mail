//! PASS/FAIL collector. Every check prints one line as it happens.

pub struct Report {
    rows: Vec<(String, bool, String)>,
}

impl Report {
    pub fn new() -> Self {
        Report { rows: Vec::new() }
    }
    pub fn check(&mut self, id: &str, ok: bool, detail: impl Into<String>) {
        let detail = detail.into();
        println!("{} {id:<34} {detail}", if ok { "PASS" } else { "FAIL" });
        self.rows.push((id.to_string(), ok, detail));
    }
    /// Observations that are not pass/fail (measurements, library gaps reproduced).
    pub fn info(&mut self, id: &str, detail: impl Into<String>) {
        println!("INFO {id:<34} {}", detail.into());
    }
    pub fn failed(&self) -> usize {
        self.rows.iter().filter(|r| !r.1).count()
    }
    pub fn summary(&self) {
        println!(
            "\n{} checks, {} passed, {} failed",
            self.rows.len(),
            self.rows.len() - self.failed(),
            self.failed()
        );
        for (id, _, d) in self.rows.iter().filter(|r| !r.1) {
            println!("  FAIL {id}: {d}");
        }
    }
}
