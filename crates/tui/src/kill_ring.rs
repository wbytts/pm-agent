#[derive(Debug, Clone, Default)]
pub struct KillRing {
    ring: Vec<String>,
}

impl KillRing {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, text: impl Into<String>, prepend: bool, accumulate: bool) {
        let text = text.into();
        if text.is_empty() {
            return;
        }

        if accumulate && !self.ring.is_empty() {
            let last = self.ring.pop().expect("ring is not empty");
            self.ring.push(if prepend {
                format!("{text}{last}")
            } else {
                format!("{last}{text}")
            });
        } else {
            self.ring.push(text);
        }
    }

    pub fn peek(&self) -> Option<&str> {
        self.ring.last().map(String::as_str)
    }

    pub fn rotate(&mut self) {
        if self.ring.len() > 1 {
            let last = self.ring.pop().expect("ring length checked");
            self.ring.insert(0, last);
        }
    }

    pub fn len(&self) -> usize {
        self.ring.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ring.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kill_ring_accumulates_and_rotates() {
        let mut ring = KillRing::new();
        ring.push("hello", false, false);
        ring.push(" world", false, true);
        ring.push("older", false, false);
        assert_eq!(ring.peek(), Some("older"));
        ring.rotate();
        assert_eq!(ring.peek(), Some("hello world"));
    }
}
