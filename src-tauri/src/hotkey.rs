use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{Duration, Instant};

const CHORD_DELAY: Duration = Duration::from_millis(180);
const IDLE: u8 = 0;
const PROCESSING: u8 = 1;
const COMMITTING: u8 = 2;

pub struct ProcessingPhase(AtomicU8);

impl ProcessingPhase {
    pub fn new() -> Self {
        Self(AtomicU8::new(IDLE))
    }

    pub fn start(&self) {
        self.0.store(PROCESSING, Ordering::SeqCst);
    }

    pub fn try_cancel(&self) -> bool {
        self.0
            .compare_exchange(PROCESSING, IDLE, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    pub fn try_commit(&self) -> bool {
        self.0
            .compare_exchange(PROCESSING, COMMITTING, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    pub fn finish(&self) {
        self.0.store(IDLE, Ordering::SeqCst);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TriggerMode {
    Hold,
    Toggle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Key {
    Trigger,
    Shift,
    Space,
    Escape,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action {
    BeginDictate,
    BeginTranslate,
    BeginAsk,
    Finish,
    Cancel,
}

pub struct HotkeyMachine {
    mode: TriggerMode,
    trigger_down: bool,
    interaction_started: bool,
    recording_active: bool,
    pending_base_press: Option<Instant>,
}

impl HotkeyMachine {
    pub fn new(mode: TriggerMode) -> Self {
        Self {
            mode,
            trigger_down: false,
            interaction_started: false,
            recording_active: false,
            pending_base_press: None,
        }
    }

    pub fn reset(&mut self) {
        self.trigger_down = false;
        self.interaction_started = false;
        self.recording_active = false;
        self.pending_base_press = None;
    }

    pub fn key_down(&mut self, key: Key, now: Instant) -> Option<Action> {
        match key {
            Key::Trigger if !self.trigger_down => {
                self.trigger_down = true;
                self.pending_base_press = Some(now + CHORD_DELAY);
                None
            }
            Key::Shift if self.trigger_down => self.start_mode(Action::BeginTranslate),
            Key::Space if self.trigger_down => self.start_mode(Action::BeginAsk),
            Key::Escape if self.recording_active => {
                self.reset();
                Some(Action::Cancel)
            }
            Key::Other if self.trigger_down && self.pending_base_press.is_some() => {
                self.trigger_down = false;
                self.pending_base_press = None;
                self.interaction_started = false;
                None
            }
            _ => None,
        }
    }

    pub fn key_up(&mut self, key: Key) -> Option<Action> {
        if key != Key::Trigger || !self.trigger_down {
            return None;
        }
        self.trigger_down = false;
        let action = match self.mode {
            TriggerMode::Hold if self.interaction_started => {
                self.recording_active = false;
                Some(Action::Finish)
            }
            TriggerMode::Toggle
                if !self.interaction_started && self.pending_base_press.is_some() =>
            {
                self.start_mode(Action::BeginDictate)
            }
            _ => None,
        };
        self.pending_base_press = None;
        self.interaction_started = false;
        action
    }

    pub fn tick(&mut self, now: Instant) -> Option<Action> {
        if self
            .pending_base_press
            .is_some_and(|deadline| now >= deadline)
        {
            return self.start_mode(Action::BeginDictate);
        }
        None
    }

    fn start_mode(&mut self, mode: Action) -> Option<Action> {
        if self.interaction_started {
            return None;
        }
        self.pending_base_press = None;
        self.interaction_started = true;
        if self.mode == TriggerMode::Toggle && self.recording_active {
            self.recording_active = false;
            return Some(Action::Finish);
        }
        self.recording_active = true;
        Some(mode)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_and_commit_cannot_both_win() {
        let phase = ProcessingPhase::new();
        phase.start();
        assert!(phase.try_cancel());
        assert!(!phase.try_commit());
        phase.start();
        assert!(phase.try_commit());
        assert!(!phase.try_cancel());
        phase.finish();
    }

    #[test]
    fn hold_starts_after_chord_window_and_ends_on_release() {
        let now = Instant::now();
        let mut machine = HotkeyMachine::new(TriggerMode::Hold);
        assert_eq!(machine.key_down(Key::Trigger, now), None);
        assert_eq!(machine.tick(now + Duration::from_millis(179)), None);
        assert_eq!(machine.tick(now + CHORD_DELAY), Some(Action::BeginDictate));
        assert_eq!(machine.key_up(Key::Trigger), Some(Action::Finish));
    }

    #[test]
    fn quick_toggle_tap_starts_and_next_tap_stops() {
        let now = Instant::now();
        let mut machine = HotkeyMachine::new(TriggerMode::Toggle);
        assert_eq!(machine.key_down(Key::Trigger, now), None);
        assert_eq!(machine.key_up(Key::Trigger), Some(Action::BeginDictate));
        assert_eq!(machine.key_down(Key::Trigger, now), None);
        assert_eq!(machine.key_up(Key::Trigger), Some(Action::Finish));
    }

    #[test]
    fn chords_preempt_dictation_without_starting_twice() {
        let now = Instant::now();
        let mut machine = HotkeyMachine::new(TriggerMode::Hold);
        machine.key_down(Key::Trigger, now);
        assert_eq!(
            machine.key_down(Key::Shift, now),
            Some(Action::BeginTranslate)
        );
        assert_eq!(machine.tick(now + CHORD_DELAY), None);
        assert_eq!(machine.key_down(Key::Space, now), None);
        assert_eq!(machine.key_up(Key::Trigger), Some(Action::Finish));
    }

    #[test]
    fn escape_cancels_active_session() {
        let now = Instant::now();
        let mut machine = HotkeyMachine::new(TriggerMode::Hold);
        machine.key_down(Key::Trigger, now);
        machine.tick(now + CHORD_DELAY);
        assert_eq!(machine.key_down(Key::Escape, now), Some(Action::Cancel));
        assert_eq!(machine.key_up(Key::Trigger), None);
    }

    #[test]
    fn another_shortcut_key_cancels_pending_trigger() {
        let now = Instant::now();
        let mut machine = HotkeyMachine::new(TriggerMode::Hold);
        machine.key_down(Key::Trigger, now);
        assert_eq!(machine.key_down(Key::Other, now), None);
        assert_eq!(machine.tick(now + CHORD_DELAY), None);
        assert_eq!(machine.key_up(Key::Trigger), None);
    }
}
