//! Prefetch policy: decide which pages to warm before a scan reaches them.

/// Prefetch advice for one group: pages worth warming, in order.
pub fn advise(page_count: usize, budget: usize) -> Vec<usize> {
    let warm = page_count.min(budget.max(1));
    (0..warm).collect()
}

/// Whether the next group should warm concurrently (small groups only:
/// large groups saturate the loader on their own).
pub fn concurrent_warm(page_count: usize, page_bytes: usize) -> bool {
    page_count <= 4 && page_bytes <= 1 << 20
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advice_respects_budget() {
        assert_eq!(advise(10, 3), vec![0, 1, 2]);
        assert_eq!(advise(2, 8), vec![0, 1]);
        assert!(concurrent_warm(3, 1 << 16));
        assert!(!concurrent_warm(9, 1 << 16));
        assert!(!concurrent_warm(3, 1 << 24));
    }
}
