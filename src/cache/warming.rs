//! Warm-up planner: given a scan plan, decide which pages to warm and in
//! what order so the first scan never stalls on cold decodes.

use crate::format::FileImage;

/// A warm-up plan for one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WarmPlan {
    /// (group, page index within group) in warm order.
    pub order: Vec<(usize, usize)>,
    pub total_bytes: u64,
}

/// Plan warming for the groups a `[lo, hi]` scan will touch.
pub fn plan_warm(image: &FileImage<'_>, lo: i64, hi: i64, budget_bytes: u64) -> WarmPlan {
    let mut order = Vec::new();
    let mut total = 0u64;
    for (gi, group) in image.groups.iter().enumerate() {
        let zone = image.zones[gi];
        if zone.min > hi || lo > zone.max {
            continue;
        }
        for (pi, page) in group.pages.iter().enumerate() {
            if total + page.len as u64 > budget_bytes {
                return WarmPlan {
                    order,
                    total_bytes: total,
                };
            }
            order.push((gi, pi));
            total += page.len as u64;
        }
    }
    WarmPlan {
        order,
        total_bytes: total,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::{FileWriter, ZoneMap};
    use crate::page::{plain, write_page};
    use crate::schema::tags;

    #[test]
    fn plans_only_matching_groups() {
        let mut w = FileWriter::new(crate::format::VERSION_V2);
        w.column(1, "a", tags::TYPE_I64, tags::ENC_PLAIN, false);
        let p1 = write_page(tags::ENC_PLAIN, 2, 0, &plain::write_i64(&[1, 2]));
        let p2 = write_page(tags::ENC_PLAIN, 2, 0, &plain::write_i64(&[90, 99]));
        w.row_group(
            2,
            vec![(p1, 2, tags::ENC_PLAIN)],
            ZoneMap {
                min: 1,
                max: 2,
                rows: 2,
            },
        );
        w.row_group(
            2,
            vec![(p2, 2, tags::ENC_PLAIN)],
            ZoneMap {
                min: 90,
                max: 99,
                rows: 2,
            },
        );
        let bytes = w.finish();
        let image = crate::format::open(&bytes).unwrap();
        let plan = plan_warm(&image, 90, 100, 1 << 20);
        assert_eq!(plan.order, vec![(1, 0)]);
        assert!(plan.total_bytes > 0);
    }
}
