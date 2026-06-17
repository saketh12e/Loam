#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    let _ = loam::view::run_view_driver(data);
});
