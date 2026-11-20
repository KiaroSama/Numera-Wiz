//! One due picture and a settlement result for the native workspace.
use nw_domain::time::Rational;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

pub struct NativePictureFrame {
    pub width: usize,
    pub height: usize,
    pub at: Rational,
    pub rgba: Vec<u8>,
}

#[derive(Default)]
struct Slot {
    frame: Option<NativePictureFrame>,
    outcome: Option<Result<(), String>>,
}

#[derive(Clone, Default)]
pub struct NativePictureMailbox {
    slot: Arc<Mutex<Slot>>,
    cancelled: Arc<AtomicBool>,
}

impl NativePictureMailbox {
    pub fn publish(&self, frame: NativePictureFrame) {
        if !self.cancelled() {
            self.slot
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .frame = Some(frame);
        }
    }

    pub fn finish(&self, result: Result<(), String>) {
        self.slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .outcome = Some(result);
    }

    pub(super) fn take(&self) -> (Option<NativePictureFrame>, Option<Result<(), String>>) {
        let mut slot = self
            .slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        (slot.frame.take(), slot.outcome.take())
    }

    pub fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    pub(super) fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
}
