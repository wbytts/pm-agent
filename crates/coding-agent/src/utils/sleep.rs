use std::time::Duration;

pub fn sleep(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn sleeps_for_requested_duration() {
        let started = Instant::now();
        sleep(1);
        assert!(started.elapsed() >= Duration::from_millis(1));
    }
}
