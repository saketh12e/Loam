#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    let _ = loam::session::transactions::run_txn_driver(data);
});
