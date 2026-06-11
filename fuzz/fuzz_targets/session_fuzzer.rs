#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    let _ = loam::session::run_session_driver(data);
});
