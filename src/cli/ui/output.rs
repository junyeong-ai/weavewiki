use console::style;

pub struct Output;

impl Output {
    pub fn new() -> Self {
        Self
    }

    pub fn success(&self, message: &str) {
        println!("{} {}", style("✓").green(), message);
    }

    pub fn error(&self, message: &str) {
        eprintln!("{} {}", style("✗").red(), message);
    }

    pub fn warning(&self, message: &str) {
        println!("{} {}", style("⚠").yellow(), message);
    }

    pub fn info(&self, message: &str) {
        println!("{} {}", style("ℹ").blue(), message);
    }

    pub fn header(&self, message: &str) {
        println!("\n{}", style(message).bold().underlined());
    }

    pub fn section(&self, message: &str) {
        println!("\n{}", style(message).bold());
        println!("{}", "─".repeat(40));
    }
}

impl Default for Output {
    fn default() -> Self {
        Self::new()
    }
}
