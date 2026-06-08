use std::sync::atomic::{AtomicBool, Ordering};

static STDOUT_TAKEN_OVER: AtomicBool = AtomicBool::new(false);

pub fn take_over_stdout() {
    STDOUT_TAKEN_OVER.store(true, Ordering::SeqCst);
}

pub fn restore_stdout() {
    STDOUT_TAKEN_OVER.store(false, Ordering::SeqCst);
}

pub fn is_stdout_taken_over() -> bool {
    STDOUT_TAKEN_OVER.load(Ordering::SeqCst)
}

pub fn write_raw_stdout(text: &str) {
    print!("{text}");
}

pub fn flush_raw_stdout() -> std::io::Result<()> {
    use std::io::Write;
    std::io::stdout().flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_stdout_takeover_state() {
        restore_stdout();
        assert!(!is_stdout_taken_over());
        take_over_stdout();
        assert!(is_stdout_taken_over());
        restore_stdout();
    }
}
