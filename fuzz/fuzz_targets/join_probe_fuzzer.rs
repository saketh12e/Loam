#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    let _ = loam::exec::join::run_join_driver(data);
});
