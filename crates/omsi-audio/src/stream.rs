//! A buffer of sound that is filled while it plays: the decoded internet radio. A reader
//! thread pushes stereo frames at the stream's own rate; the mixer takes them at the
//! device rate (see `AudioEngine::play_stream`).

use parking_lot::{Mutex, MutexGuard};
use crate::fx::{FxState, RadioFx};
use std::collections::VecDeque;

/// What the radio asks of the mixer for this stream, set every frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StreamControl {
    /// Where the sound sits between the ears, -1 (left) .. 1 (right): the far ear is turned
    /// down by up to 40 %, the near one stays.
    pub balance: f32,
    /// The optional radio effects (all off by default).
    pub fx: RadioFx,
}

pub struct StreamBuf {
    inner: Mutex<StreamInner>,
    /// What the reader says about the stream: the song playing, or why it is silent.
    status: Mutex<String>,
    /// What the station says about itself: (its name from the response header, the song or
    /// whatever text it puts in the stream title). Either may be empty.
    info: Mutex<(String, String)>,
}

pub struct StreamInner {
    /// Frames per second of what is in the buffer.
    pub rate: u32,
    frames: VecDeque<[f32; 2]>,
    /// Playing (else filling up: at the start and after the network fell behind).
    playing: bool,
    /// How much is gathered before playing starts again after a stall, in seconds: a
    /// connection that stalled once tends to stall again, so each stall asks for more.
    prebuffer: f32,
    /// The voice ends (the radio was switched off or to another station).
    pub closed: bool,
    pub ctl: StreamControl,
    pub(crate) fx: FxState,
}

impl StreamInner {
    /// The frame playing now and the next one, while there are both.
    pub(crate) fn pair(&mut self) -> Option<([f32; 2], [f32; 2])> {
        if !self.playing {
            if (self.frames.len() as f32) < self.rate as f32 * self.prebuffer {
                return None;
            }
            self.playing = true;
        }
        if self.frames.len() < 2 {
            self.playing = false;
            self.prebuffer = (self.prebuffer + 1.0).min(6.0);
            return None;
        }
        Some((self.frames[0], self.frames[1]))
    }

    pub(crate) fn advance(&mut self) {
        self.frames.pop_front();
    }
}

impl Default for StreamBuf {
    fn default() -> Self {
        StreamBuf {
            inner: Mutex::new(StreamInner {
                rate: 44100,
                frames: VecDeque::new(),
                playing: false,
                prebuffer: 1.5,
                closed: false,
                ctl: StreamControl::default(),
                fx: FxState::default(),
            }),
            status: Mutex::new(String::new()),
            info: Mutex::new((String::new(), String::new())),
        }
    }
}

impl StreamBuf {
    pub(crate) fn lock(&self) -> MutexGuard<'_, StreamInner> {
        self.inner.lock()
    }

    /// Add decoded frames at `rate`. A new rate (another station, a stream that changed
    /// format) starts the buffer over.
    pub fn push(&self, rate: u32, frames: impl IntoIterator<Item = [f32; 2]>) {
        let mut b = self.inner.lock();
        if b.rate != rate {
            b.rate = rate;
            b.frames.clear();
            b.playing = false;
        }
        b.frames.extend(frames);
    }

    /// Seconds of sound waiting to be played.
    pub fn buffered(&self) -> f32 {
        let b = self.inner.lock();
        b.frames.len() as f32 / b.rate.max(1) as f32
    }

    /// Whether the mixer is playing it (not waiting for the buffer to fill).
    pub fn is_playing(&self) -> bool {
        self.inner.lock().playing
    }

    /// Balance and effects for what the mixer plays next.
    pub fn set_control(&self, ctl: StreamControl) {
        self.inner.lock().ctl = ctl;
    }

    pub fn close(&self) {
        let mut b = self.inner.lock();
        b.closed = true;
        b.frames.clear();
    }

    pub fn set_status(&self, s: impl Into<String>) {
        *self.status.lock() = s.into();
    }

    /// The station's own name and stream title, as the reader found them.
    pub fn set_info(&self, name: &str, song: &str) {
        let mut i = self.info.lock();
        if i.0 != name || i.1 != song {
            *i = (name.to_string(), song.to_string());
        }
    }

    /// (the station's name, the stream title) - empty where the station says none.
    pub fn info(&self) -> (String, String) {
        self.info.lock().clone()
    }

    pub fn status(&self) -> String {
        self.status.lock().clone()
    }

    pub fn is_closed(&self) -> bool {
        self.inner.lock().closed
    }
}
