//! Latest-wins loading mailbox and generation tracking, independent of audio
//! hardware so cancellation/races can be tested deterministically.

use std::sync::{Condvar, Mutex};

pub(crate) struct Latest<T> {
    state: Mutex<(Option<T>, bool)>,
    ready: Condvar,
}

impl<T> Default for Latest<T> {
    fn default() -> Self {
        Self {
            state: Mutex::new((None, false)),
            ready: Condvar::new(),
        }
    }
}

impl<T> Latest<T> {
    pub fn replace(&self, value: T) {
        let mut state = self.state.lock().unwrap();
        if !state.1 {
            state.0 = Some(value);
        }
        self.ready.notify_one();
    }

    pub fn clear(&self) {
        self.state.lock().unwrap().0 = None;
    }

    pub fn take(&self) -> Option<T> {
        let mut state = self.state.lock().unwrap();
        loop {
            if state.1 {
                return None;
            }
            if let Some(value) = state.0.take() {
                return Some(value);
            }
            state = self.ready.wait(state).unwrap();
        }
    }

    pub fn close(&self) {
        *self.state.lock().unwrap() = (None, true);
        self.ready.notify_all();
    }
}

#[derive(Default)]
pub(crate) struct LoadState {
    generation: u64,
    pub pending: bool,
    pub paused: bool,
}

impl LoadState {
    pub fn begin(&mut self) -> u64 {
        self.generation = self.generation.wrapping_add(1);
        self.pending = true;
        self.paused = false;
        self.generation
    }

    pub fn cancel(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.pending = false;
        self.paused = false;
    }

    pub fn complete(&mut self, generation: u64) -> bool {
        if !self.pending || generation != self.generation {
            return false;
        }
        self.pending = false;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_discards_late_success_and_failure() {
        let mut state = LoadState::default();
        let generation = state.begin();
        state.cancel();
        assert!(!state.complete(generation));
        assert!(!state.pending);
    }

    #[test]
    fn newer_source_wins() {
        let mut state = LoadState::default();
        let old = state.begin();
        let new = state.begin();
        assert!(!state.complete(old));
        assert!(state.complete(new));
        assert!(!state.complete(new));
    }

    #[test]
    fn pause_is_preserved_when_loading_finishes() {
        let mut state = LoadState::default();
        let generation = state.begin();
        state.paused = true;
        assert!(state.complete(generation));
        assert!(state.paused);
        state.begin();
        assert!(!state.paused);
    }

    #[test]
    fn mailbox_keeps_only_the_latest_request() {
        let mailbox = Latest::default();
        mailbox.replace("old");
        mailbox.replace("new");
        assert_eq!(mailbox.take(), Some("new"));
        mailbox.replace("discard");
        mailbox.clear();
        mailbox.close();
        mailbox.replace("too late");
        assert_eq!(mailbox.take(), None);
    }

    #[test]
    fn closing_wakes_the_waiting_loader() {
        let mailbox = std::sync::Arc::new(Latest::<()>::default());
        let loader = std::sync::Arc::clone(&mailbox);
        let thread = std::thread::spawn(move || loader.take());
        mailbox.close();
        assert_eq!(thread.join().unwrap(), None);
    }
}
