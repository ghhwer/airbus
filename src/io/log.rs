use std::io::{self, Write};
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Level {
    Debug = 0,
    Info = 1,
    Warn = 2,
    Error = 3,
}

static LEVEL: AtomicU8 = AtomicU8::new(Level::Info as u8);

fn name(level: Level) -> &'static str {
    match level {
        Level::Debug => "debug",
        Level::Info => "info",
        Level::Warn => "warn",
        Level::Error => "error",
    }
}

pub fn set_level(level: Level) {
    LEVEL.store(level as u8, Ordering::Relaxed);
}

pub fn level() -> Level {
    match LEVEL.load(Ordering::Relaxed) {
        0 => Level::Debug,
        1 => Level::Info,
        2 => Level::Warn,
        _ => Level::Error,
    }
}

fn write(msg_level: Level, msg: &str) {
    if msg_level < level() {
        return;
    }
    let _ = writeln!(io::stderr(), "[{}] {}", name(msg_level), msg);
    let _ = io::stderr().flush();
}

pub fn debug(msg: &str) {
    write(Level::Debug, msg);
}

pub fn info(msg: &str) {
    write(Level::Info, msg);
}

pub fn warn(msg: &str) {
    write(Level::Warn, msg);
}

pub fn error(msg: &str) {
    write(Level::Error, msg);
}
