//! View registry and view-driver coverage.

use loam::dict::{DictRegistry, Dictionary};
use loam::view::{run_view_driver, write_view_input, ViewRegistry};

#[test]
fn many_views_tracked() {
    let mut dicts = DictRegistry::default();
    dicts.install(Dictionary::new(1, 1, vec![b"a".to_vec()]));
    let mut views = ViewRegistry::default();
    let mut ids = Vec::new();
    for i in 0..12u32 {
        ids.push(views.create(1, 1, &format!("q{i}")));
    }
    assert_eq!(views.len(), 12);
    for id in ids {
        views.invalidate(id, &mut dicts).unwrap();
    }
    assert!(views.is_empty());
}

#[test]
fn view_driver_full_cycle() {
    let input = write_view_input(
        1,
        &[b"red", b"green", b"blue"],
        &["a > 1", "a > 2", "a > 3"],
    );
    assert_eq!(run_view_driver(&input).unwrap(), 3);
}

#[test]
fn view_driver_bounds() {
    assert!(run_view_driver(&write_view_input(1, &[], &["q"])).is_err());
    let too_many: Vec<&str> = (0..100).map(|_| "q").collect();
    assert!(run_view_driver(&write_view_input(1, &[b"a"], &too_many)).is_err());
}

#[test]
fn invalidating_twice_errors() {
    let mut dicts = DictRegistry::default();
    dicts.install(Dictionary::new(1, 1, vec![b"a".to_vec()]));
    let mut views = ViewRegistry::default();
    let id = views.create(1, 1, "q");
    views.invalidate(id, &mut dicts).unwrap();
    assert!(views.invalidate(id, &mut dicts).is_err());
}
