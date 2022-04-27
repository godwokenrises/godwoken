use std::time::Duration;

use rand::{thread_rng, Rng};

pub struct ExponentialBackoff {
    base: Duration,
    current_multiplier: f32,
    multiplier: f32,
    max_sleep: Duration,
    jitter: bool,
}

impl ExponentialBackoff {
    pub fn new(base: Duration) -> Self {
        Self {
            base,
            current_multiplier: 1.0,
            multiplier: 2.0,
            max_sleep: base * 32,
            jitter: true,
        }
    }

    pub fn next_sleep(&mut self) -> Duration {
        let t = self.base.mul_f32(self.current_multiplier);
        let t = if t >= self.max_sleep {
            self.max_sleep
        } else {
            self.current_multiplier *= self.multiplier;
            t
        };
        if self.jitter {
            // https://aws.amazon.com/cn/blogs/architecture/exponential-backoff-and-jitter/
            thread_rng().gen_range(Duration::ZERO..t)
        } else {
            t
        }
    }

    pub fn reset(&mut self) {
        self.current_multiplier = 1.0;
    }

    pub fn with_multiplier(self, multiplier: f32) -> Self {
        Self { multiplier, ..self }
    }

    pub fn with_max_sleep(self, max_sleep: Duration) -> Self {
        Self { max_sleep, ..self }
    }

    pub fn with_jitter(self, jitter: bool) -> Self {
        Self { jitter, ..self }
    }
}

#[cfg(test)]
#[test]
fn test_backoff() {
    let mut b =
        ExponentialBackoff::new(Duration::from_secs(1)).with_max_sleep(Duration::from_secs(64));
    b.next_sleep();
    assert!(b.current_multiplier == 2.0);
    b.next_sleep();
    assert!(b.current_multiplier == 4.0);
    for _ in 0..10 {
        b.next_sleep();
    }
    assert!(b.current_multiplier == 64.0);
}
