//! Join variants, window aggregates, and order-by flows.

use loam::exec::join::{anti_join_count, inner_join_count, left_join_count, semi_join_count};
use loam::exec::sort::{self, Direction};
use loam::exec::window::{dense_rank, lag, moving_avg, moving_sum};

#[test]
fn join_family_counts() {
    let left = vec![1, 2, 2, 3, 5];
    let right = vec![2, 5];
    assert_eq!(inner_join_count(&left, &right), 3);
    assert_eq!(left_join_count(&left, &right), 5);
    assert_eq!(semi_join_count(&left, &right), 3);
    assert_eq!(anti_join_count(&left, &right), 2);
}

#[test]
fn window_aggregates_flow() {
    let values = vec![2, 4, 6, 8, 10];
    assert_eq!(moving_sum(&values, 2).unwrap(), vec![2, 6, 10, 14, 18]);
    assert_eq!(moving_avg(&values, 2).unwrap(), vec![2, 3, 5, 7, 9]);
    assert_eq!(lag(&values, 1, 0), vec![0, 2, 4, 6, 8]);
    assert_eq!(dense_rank(&[1, 1, 3, 3, 7]), vec![1, 1, 2, 2, 3]);
}

#[test]
fn order_by_top_k() {
    let out = sort::order_by(&[42, 7, 19, 3, 11], Direction::Desc, Some(3)).unwrap();
    assert_eq!(out, vec![42, 19, 11]);
}

#[test]
fn merge_sorted_runs_flow() {
    let merged = sort::merge_sorted(&[1, 4, 9], &[2, 3, 10]);
    assert_eq!(merged, vec![1, 2, 3, 4, 9, 10]);
}
