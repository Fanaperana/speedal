mod capture;
mod runtime;
mod wav;

pub use capture::{
    handle_event, listen_loop, listen_until, open_system_loopback, permission_hint,
};
pub use flexaudio::{Event as CaptureEvent, Stream};
pub use runtime::execution_config;
pub use wav::{Audio, load_wav};

pub const TARGET_HZ: u32 = 16_000;
