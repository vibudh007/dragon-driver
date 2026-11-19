use std::sync::{Condvar, Mutex};
use std::time::Duration;

#[derive(Default)]
struct State {
    running: bool,
    generation: u64,
}

#[derive(Default)]
pub struct MicroTimer {
    state: Mutex<State>,
    changed: Condvar,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WaitOutcome {
    Elapsed,
    Stopped,
}

impl MicroTimer {
    pub fn start(&self) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.running = true;
    }

    pub fn wait(&self, microseconds: u32) -> WaitOutcome {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if !state.running {
            return WaitOutcome::Stopped;
        }
        let generation = state.generation;
        let (state, _) = self
            .changed
            .wait_timeout_while(
                state,
                Duration::from_micros(u64::from(microseconds)),
                |state| state.running && state.generation == generation,
            )
            .unwrap_or_else(|error| error.into_inner());
        if state.running && state.generation == generation {
            WaitOutcome::Elapsed
        } else {
            WaitOutcome::Stopped
        }
    }

    pub fn stop(&self) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.running = false;
        state.generation = state.generation.wrapping_add(1);
        self.changed.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn waits_only_while_running() {
        let timer = MicroTimer::default();
        assert_eq!(timer.wait(u32::MAX), WaitOutcome::Stopped);
        timer.start();
        assert_eq!(timer.wait(0), WaitOutcome::Elapsed);
        timer.stop();
        assert_eq!(timer.wait(u32::MAX), WaitOutcome::Stopped);
    }

    #[test]
    fn blocking_wait_can_be_stopped() {
        let timer = MicroTimer::default();
        timer.start();
        thread::scope(|scope| {
            let waiting = scope.spawn(|| timer.wait(1_000_000));
            timer.stop();
            assert_eq!(waiting.join().unwrap(), WaitOutcome::Stopped);
        });
    }
}
