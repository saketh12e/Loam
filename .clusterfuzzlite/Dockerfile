FROM gcr.io/oss-fuzz-base/base-builder-rust

# The build is fully offline: crate sources are vendored in vendor/ and
# .cargo/config.toml points cargo at them.
COPY . $SRC/loam
WORKDIR $SRC/loam

RUN .clusterfuzzlite/build.sh
