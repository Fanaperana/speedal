mod capture;
mod wav;

pub use capture::{handle_event, listen_loop, open_system_loopback, permission_hint};
pub use wav::{Audio, load_wav};

pub const TARGET_HZ: u32 = 16_000;
