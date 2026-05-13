//! Embedded user binaries. Phase 5d/e stand-in for a filesystem.

pub const INITCODE: &[u8] = include_bytes!(env!("INITCODE_BIN_PATH"));
const ECHO: &[u8] = include_bytes!(env!("ECHO_BIN_PATH"));
const HELLO: &[u8] = include_bytes!(env!("HELLO_BIN_PATH"));
const PIPETEST: &[u8] = include_bytes!(env!("PIPETEST_BIN_PATH"));

const BINS: &[(&str, &[u8])] = &[
    ("/echo", ECHO),
    ("/hello", HELLO),
    ("/pipetest", PIPETEST),
];

pub fn find(path: &str) -> Option<&'static [u8]> {
    BINS.iter()
        .find(|(name, _)| *name == path)
        .map(|(_, bytes)| *bytes)
}
