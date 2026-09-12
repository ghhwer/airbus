use std::cmp::Ordering;
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use std::cell::Cell;

#[derive(Clone, Copy, Eq, PartialEq, Hash)]
pub struct UuidV7 {
    bytes: [u8; 16],
}

thread_local! {
    static RNG_STATE: Cell<u64> = const { Cell::new(0x9E37_79B9_7F4A_7C15) };
}

fn next_u64() -> u64 {
    RNG_STATE.with(|state| {
        let mut x = state.get();
        if x == 0 {
            let seed = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(1);
            x = seed ^ 0xA076_1D64_78BD_642F;
            if x == 0 {
                x = 1;
            }
        }
        // xorshift64*
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        state.set(x);
        x
    })
}

impl UuidV7 {
    pub fn generate() -> Self {
        let ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        let rand_a = next_u64();
        let rand_b = next_u64();

        let mut bytes = [0u8; 16];
        bytes[0] = ((ms >> 40) & 0xFF) as u8;
        bytes[1] = ((ms >> 32) & 0xFF) as u8;
        bytes[2] = ((ms >> 24) & 0xFF) as u8;
        bytes[3] = ((ms >> 16) & 0xFF) as u8;
        bytes[4] = ((ms >> 8) & 0xFF) as u8;
        bytes[5] = (ms & 0xFF) as u8;

        let rand_a_12 = ((rand_a >> 52) & 0x0FFF) as u16;
        bytes[6] = 0x70 | (((rand_a_12 >> 8) & 0x0F) as u8);
        bytes[7] = (rand_a_12 & 0xFF) as u8;

        bytes[8] = 0x80 | (((rand_b >> 56) & 0x3F) as u8);
        bytes[9] = ((rand_b >> 48) & 0xFF) as u8;
        bytes[10] = ((rand_b >> 40) & 0xFF) as u8;
        bytes[11] = ((rand_b >> 32) & 0xFF) as u8;
        bytes[12] = ((rand_b >> 24) & 0xFF) as u8;
        bytes[13] = ((rand_b >> 16) & 0xFF) as u8;
        bytes[14] = ((rand_b >> 8) & 0xFF) as u8;
        bytes[15] = (rand_b & 0xFF) as u8;

        Self { bytes }
    }
}

impl Ord for UuidV7 {
    fn cmp(&self, other: &Self) -> Ordering {
        self.bytes.cmp(&other.bytes)
    }
}

impl PartialOrd for UuidV7 {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for UuidV7 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            self.bytes[0],
            self.bytes[1],
            self.bytes[2],
            self.bytes[3],
            self.bytes[4],
            self.bytes[5],
            self.bytes[6],
            self.bytes[7],
            self.bytes[8],
            self.bytes[9],
            self.bytes[10],
            self.bytes[11],
            self.bytes[12],
            self.bytes[13],
            self.bytes[14],
            self.bytes[15],
        )
    }
}

impl fmt::Debug for UuidV7 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl std::str::FromStr for UuidV7 {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let clean: String = s.chars().filter(|&c| c != '-').collect();
        if clean.len() != 32 {
            return Err(format!("invalid uuid length: {}", s.len()));
        }
        if !clean.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("invalid hex in uuid".to_string());
        }
        let mut bytes = [0u8; 16];
        for i in 0..16 {
            bytes[i] = u8::from_str_radix(&clean[i * 2..i * 2 + 2], 16)
                .map_err(|e| format!("invalid hex in uuid: {e}"))?;
        }
        Ok(Self { bytes })
    }
}

pub fn generate_uuidv7() -> UuidV7 {
    UuidV7::generate()
}
