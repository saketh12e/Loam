# loam

`loam` is an embedded columnar analytics engine. It reads and writes its own
`.loam` file format, decodes compressed pages (RLE, dictionary, delta, bitpack),
and evaluates filters and aggregations through a vectorized operator pipeline.

## Highlights

- Self-contained `.loam` columnar format with zone maps for row-group pruning
- Page encodings: plain, RLE (with cross-page continuation), dictionary,
  delta, bitpack, and a hybrid dictionary/RLE stripe layout
- Expression language compiled to closures, with range relaxation driven by
  late-arriving column statistics
- Vectorized execution: scan, filter, project, group-by, join, limit
- Memory-pressure spill to segment files with reload and resume
- Page cache with pin epochs and clock eviction
- Materialized views subscribed to dictionary generations
- Session scripts for embedded multi-operation workflows

## Example

```rust
let mut engine = loam::Engine::new();
let image = engine.open_bytes(&file_bytes)?;
let ranges = engine.plan_scan(&image, 15, 40)?;
```

## Layout

- `format` — file header, section directory, row groups, footer
- `page` — page headers and decoding kernels
- `batch` — record batches, zero-copy slices, stripe expansion
- `arena` — scratch allocation for decode and operators
- `dict` — per-column dictionaries and generations
- `cache` — page cache with pin epochs
- `expr` / `plan` / `exec` — expression language, plans, pipeline
- `spill` — segment spill and reload
- `view` / `session` — materialized views and session scripts

## Status

Private, pre-1.0. The on-disk format is versioned (`V1`, `V2`); mixed-version
files are accepted through the negotiation layer.
