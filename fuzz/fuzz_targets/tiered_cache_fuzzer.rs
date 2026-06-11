#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    let _ = loam::cache::tiered::run_tiered_driver(data);
});
