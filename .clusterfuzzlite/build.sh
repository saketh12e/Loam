#!/bin/bash -eu
# Build every libFuzzer harness and install it, with its seed corpus and
# dictionary, into $OUT. Run by the ClusterFuzzLite / OSS-Fuzz pipeline
# with the repository checked out as the working directory. All crate
# sources are vendored in vendor/ so the build never touches the network.

cargo +nightly fuzz build -O --offline 2>/dev/null || CARGO_NET_OFFLINE=true cargo +nightly fuzz build -O

TRIPLE="$(rustc -vV | sed -n 's/host: //p')"
BUILD_DIR="fuzz/target/${TRIPLE}/release"

TARGETS=(
  file_open_fuzzer
  page_decode_fuzzer
  expr_eval_fuzzer
  plan_exec_fuzzer
  spill_replay_fuzzer
  session_fuzzer
  dict_load_fuzzer
  stats_ingest_fuzzer
  view_build_fuzzer
  join_probe_fuzzer
  txn_fuzzer
  tiered_cache_fuzzer
)

for target in "${TARGETS[@]}"; do
  cp "${BUILD_DIR}/${target}" "${OUT}/${target}"
  if [ -d "fuzz/corpus/${target}" ]; then
    (cd "fuzz/corpus/${target}" && zip -q -r "${OUT}/${target}_seed_corpus.zip" .)
  fi
  if [ -f fuzz/dictionary.txt ]; then
    cp fuzz/dictionary.txt "${OUT}/${target}.dict"
  fi
done
