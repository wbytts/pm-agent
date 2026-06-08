#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CountdownTick {
    Running { remaining_seconds: u64 },
    Expired,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CountdownTimer {
    remaining_seconds: u64,
    running: bool,
}

impl CountdownTimer {
    pub fn new(timeout_ms: u64) -> Self {
        Self {
            remaining_seconds: timeout_ms.div_ceil(1_000),
            running: true,
        }
    }

    pub fn remaining_seconds(&self) -> u64 {
        self.remaining_seconds
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn tick(&mut self) -> CountdownTick {
        if !self.running {
            return CountdownTick::Stopped;
        }

        self.remaining_seconds = self.remaining_seconds.saturating_sub(1);
        if self.remaining_seconds == 0 {
            self.dispose();
            CountdownTick::Expired
        } else {
            CountdownTick::Running {
                remaining_seconds: self.remaining_seconds,
            }
        }
    }

    pub fn dispose(&mut self) {
        self.running = false;
    }
}

#[cfg(test)]
mod tests {
    use super::{CountdownTick, CountdownTimer};

    #[test]
    fn countdown_timer_starts_with_ceil_seconds_like_pi() {
        let timer = CountdownTimer::new(1_001);

        assert_eq!(timer.remaining_seconds(), 2);
        assert!(timer.is_running());
    }

    #[test]
    fn countdown_timer_ticks_requests_render_and_expires_once() {
        let mut timer = CountdownTimer::new(2_000);

        assert_eq!(
            timer.tick(),
            CountdownTick::Running {
                remaining_seconds: 1
            }
        );
        assert_eq!(timer.remaining_seconds(), 1);
        assert!(timer.is_running());

        assert_eq!(timer.tick(), CountdownTick::Expired);
        assert_eq!(timer.remaining_seconds(), 0);
        assert!(!timer.is_running());

        assert_eq!(timer.tick(), CountdownTick::Stopped);
    }

    #[test]
    fn countdown_timer_dispose_stops_future_ticks() {
        let mut timer = CountdownTimer::new(3_000);

        timer.dispose();

        assert!(!timer.is_running());
        assert_eq!(timer.remaining_seconds(), 3);
        assert_eq!(timer.tick(), CountdownTick::Stopped);
    }
}
