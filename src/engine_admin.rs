//! Administrative flows: metrics and diagnostics rendered for operators.

use crate::diagnostics::Diagnostics;
use crate::metrics::Metrics;

/// One rendered admin report.
pub struct AdminReport {
    pub metrics: Metrics,
    pub events: usize,
    pub text: String,
}

/// Collect a report from an engine's telemetry.
pub fn report(metrics: &Metrics, diagnostics: &Diagnostics) -> AdminReport {
    let mut text = String::new();
    let m = metrics.snapshot();
    text.push_str(&format!(
        "files={} pages={} rows={} spills={} evictions={}\n",
        m.files_opened, m.pages_decoded, m.rows_scanned, m.spills, m.evictions
    ));
    text.push_str(&diagnostics.render());
    AdminReport {
        metrics: m,
        events: diagnostics.len(),
        text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_includes_counters_and_events() {
        let metrics = Metrics {
            files_opened: 3,
            ..Default::default()
        };
        let mut diag = Diagnostics::default();
        diag.info("opened ok");
        let report = report(&metrics, &diag);
        assert_eq!(report.metrics.files_opened, 3);
        assert_eq!(report.events, 1);
        assert!(report.text.contains("files=3"));
        assert!(report.text.contains("opened ok"));
    }
}
