//! Embedded user binaries. Phase 5c stand-in for a real filesystem —
//! `exec("/echo")` looks up the bytes here.

pub const INITCODE: &[u8] = include_bytes!(env!("INITCODE_BIN_PATH"));
const ECHO: &[u8] = include_bytes!(env!("ECHO_BIN_PATH"));
const HELLO: &[u8] = include_bytes!(env!("HELLO_BIN_PATH"));

const BINS: &[(&str, &[u8])] = &[
    ("/echo", ECHO),
    ("/hello", HELLO),
];

pub fn find(path: &str) -> Option<&'static [u8]> {
    BINS.iter()
        .find(|(name, _)| *name == path)
        .map(|(_, bytes)| *bytes)
}
