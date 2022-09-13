use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

pub struct Liveness {
    last_tick: Mutex<Instant>,
    duration: Duration,
}

impl Liveness {
    pub fn new(duration: Duration) -> Self {
        Self {
            last_tick: Mutex::new(Instant::now()),
            duration,
        }
    }

    pub fn is_live(&self) -> bool {
        self.last_tick.lock().unwrap().elapsed() < self.duration
    }

    pub fn tick(&self) {
        *self.last_tick.lock().unwrap() = Instant::now();
    }
}
