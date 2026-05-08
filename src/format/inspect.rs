//! File inspection tooling: render the structure of a `.loam` image for
//! diagnostics and support tickets.

use crate::format::FileImage;
use crate::schema::ColumnPlan;

/// One rendered line of inspection output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectLine {
    pub depth: usize,
    pub text: String,
}

/// Render an image's header, schema, groups, and zones.
pub fn inspect(image: &FileImage<'_>) -> Vec<InspectLine> {
    let mut out = Vec::new();
    let push = |out: &mut Vec<InspectLine>, depth: usize, text: String| {
        out.push(InspectLine { depth, text })
    };
    push(
        &mut out,
        0,
        format!(
            "loam v{}: {} columns, {} row groups",
            image.version,
            image.schema.columns().len(),
            image.groups.len()
        ),
    );
    for col in image.schema.columns() {
        let plan = match image.schema.negotiate(col) {
            Ok(ColumnPlan::Fast(t)) => format!("fast:{t}"),
            Ok(ColumnPlan::Mapped(t)) => format!("mapped:{t}"),
            Err(_) => "unsupported".to_string(),
        };
        push(
            &mut out,
            1,
            format!(
                "column c{} '{}' type={} enc={} chunk_v{} [{}]{}",
                col.id,
                col.name,
                col.type_tag,
                col.encoding,
                col.chunk_version,
                plan,
                if col.dictionary { " dict" } else { "" }
            ),
        );
    }
    for (i, g) in image.groups.iter().enumerate() {
        let z = image.zones[i];
        push(
            &mut out,
            1,
            format!(
                "group {i}: {} pages, footer_rows={}, zone=[{}, {}] rows={}",
                g.pages.len(),
                g.footer_rows,
                z.min,
                z.max,
                z.rows
            ),
        );
        for (j, p) in g.pages.iter().enumerate() {
            push(
                &mut out,
                2,
                format!(
                    "page {j}: enc={} rows={} bytes={} @{}",
                    p.encoding, p.rows, p.len, p.offset
                ),
            );
        }
    }
    out
}

/// Flatten inspection lines to a printable string.
pub fn render(lines: &[InspectLine]) -> String {
    let mut out = String::new();
    for line in lines {
        out.push_str(&"  ".repeat(line.depth));
        out.push_str(&line.text);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::{FileWriter, ZoneMap};
    use crate::page::{plain, write_page};
    use crate::schema::tags;

    #[test]
    fn renders_structure() {
        let mut w = FileWriter::new(crate::format::VERSION_V2);
        w.column(1, "temp", tags::TYPE_I64, tags::ENC_PLAIN, false);
        let page = write_page(tags::ENC_PLAIN, 4, 0, &plain::write_i64(&[1, 2, 3, 4]));
        w.row_group(
            4,
            vec![(page, 4, tags::ENC_PLAIN)],
            ZoneMap {
                min: 1,
                max: 4,
                rows: 4,
            },
        );
        let bytes = w.finish();
        let image = crate::format::open(&bytes).unwrap();
        let text = render(&inspect(&image));
        assert!(text.contains("loam v2"));
        assert!(text.contains("column c1 'temp'"));
        assert!(text.contains("group 0"));
    }

    #[test]
    fn inspect_marks_dictionary_columns() {
        let mut w = FileWriter::new(crate::format::VERSION_V2);
        w.column(1, "d", tags::TYPE_UTF8, tags::ENC_DICT, true);
        let page = write_page(tags::ENC_PLAIN, 1, 0, &plain::write_i64(&[1]));
        w.row_group(
            1,
            vec![(page, 1, tags::ENC_PLAIN)],
            ZoneMap {
                min: 1,
                max: 1,
                rows: 1,
            },
        );
        let bytes = w.finish();
        let image = crate::format::open(&bytes).unwrap();
        let text = render(&inspect(&image));
        assert!(text.contains("dict"));
    }
}
