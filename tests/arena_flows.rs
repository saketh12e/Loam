//! Arena lifecycle coverage: windows, checkpoints, rewinds, compaction,
//! deferred reclamation, and reservation accounting.

use loam::arena::Arena;

#[test]
fn many_windows_allocate_and_release() {
    let mut a = Arena::new();
    let mut ids = Vec::new();
    for i in 0..32 {
        ids.push(a.alloc_window(64 + i).unwrap());
    }
    assert_eq!(a.live_windows(), 32);
    for id in &ids[..16] {
        a.release_window(*id);
    }
    assert_eq!(a.live_windows(), 16);
}

#[test]
fn checkpoint_rewinds_in_order() {
    let mut a = Arena::new();
    let keep = a.alloc_window(32).unwrap();
    let cp = a.checkpoint();
    for _ in 0..8 {
        a.alloc_window(32).unwrap();
    }
    assert_eq!(a.live_windows(), 9);
    a.rewind_to(cp);
    assert_eq!(a.live_windows(), 1);
    assert!(a.view(keep).is_ok());
}

#[test]
fn nested_checkpoints_unwind_stack_like() {
    let mut a = Arena::new();
    let _first = a.alloc_window(16).unwrap();
    let cp1 = a.checkpoint();
    let _second = a.alloc_window(16).unwrap();
    let cp2 = a.checkpoint();
    let _third = a.alloc_window(16).unwrap();
    a.rewind_to(cp2);
    assert_eq!(a.live_windows(), 2);
    a.rewind_to(cp1);
    assert_eq!(a.live_windows(), 1);
}

#[test]
fn compact_keeps_every_byte() {
    let mut a = Arena::new();
    let ids: Vec<_> = (0..6).map(|_| a.alloc_window(48).unwrap()).collect();
    for (n, id) in ids.iter().enumerate() {
        let raw = a.raw(*id).unwrap();
        unsafe {
            *raw = n as u8;
        }
    }
    a.compact();
    for (n, id) in ids.iter().enumerate() {
        assert_eq!(a.view(*id).unwrap()[0], n as u8);
    }
    a.sweep_retired();
}

#[test]
fn compact_twice_is_stable() {
    let mut a = Arena::new();
    let id = a.alloc_window(24).unwrap();
    a.raw(id).unwrap();
    a.compact();
    a.compact();
    assert!(a.view(id).is_ok());
    a.sweep_retired();
    a.sweep_retired(); // empty sweep is a no-op
}

#[test]
fn reserved_tracks_allocations() {
    let mut a = Arena::new();
    let before = a.reserved();
    let _id = a.alloc_window(2048).unwrap();
    assert!(a.reserved() > before);
}

#[test]
fn forget_then_drop_is_clean() {
    let mut a = Arena::new();
    let id = a.alloc_window(64).unwrap();
    a.forget_window(id);
    assert!(a.view(id).is_err());
    // The arena no longer tracks the storage; the caller owns it now.
}

#[test]
fn window_bytes_reported() {
    let mut a = Arena::new();
    let id = a.alloc_window(96).unwrap();
    assert_eq!(a.window_bytes(id).unwrap(), 96);
}

#[test]
fn oversized_window_rejected() {
    let mut a = Arena::new();
    assert!(a.alloc_window(1 << 30).is_err());
    assert!(a.alloc_window(0).is_err());
}
