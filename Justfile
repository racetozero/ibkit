default:
    @just --list

# Format and lint all tracked files through prek (clippy and ruff check too).
fmt:
    @prek run --all-files

# Run the Rust test suite with cargo-nextest, then the doctests.
test:
    @cargo nextest run --workspace
    @cargo test --workspace --doc

# Repeat the networked tests to expose races and timing bugs.
test-stress count="30":
    @cargo nextest run --workspace --stress-count {{count}} -E 'package(ibkit) | package(ibkit-tws) | package(ibkit-fix) | package(ibkit-web)'

# The whole suite: Rust, doctests, Python and TypeScript.
test-all: test py-test node-test

# Build the Python extension into crates/ibkit-py/.venv and run pytest.
py-test:
    @cd crates/ibkit-py && uv venv -q --allow-existing --python 3.12 .venv && uv pip install -q --python .venv maturin pytest pytest-asyncio && PYO3_PYTHON=.venv/bin/python cargo clippy -q --all-targets -- -D warnings && .venv/bin/maturin develop -q && .venv/bin/pytest -q

# Build the Node addon (debug) and run the TypeScript tests with bun.
node-test:
    @cargo build -q -p ibkit-cli
    @cd crates/ibkit-node && bun install --silent && cargo clippy -q --all-targets -- -D warnings && bun run build:native:debug >/dev/null && bunx tsc -p tsconfig.json --noEmit && bun test

# Regenerate the Node client request methods from the Rust request table.
gen-node-client:
    @uv run --no-project python scripts/gen_node_client.py

# Regenerate the Python data classes and TypeScript types from the Rust defaults.
gen-python-types:
    @cargo run -q -p ibkit --example dump_defaults > target/defaults.json
    @uv run --no-project python scripts/gen_python_types.py target/defaults.json

# Print decode, encode and pipeline throughput.
bench:
    @cargo run --release -p ibkit-bench

# Run the ibkit-cli JVM tests. Needs JAVA_HOME and a display (or xvfb-run -a).
jvm:
    @scripts/jvm-smoke.sh

# Drive the ibkit TUI against a fixture config.
tui-check:
    @cargo build -q -p ibkit-cli
    @uv run scripts/check-tui.py

# Lint the Helm chart and validate each rendered config.
chart-check:
    @scripts/check-chart.sh

# Build the ibkit container image.
image:
    @docker build --target runtime -t ibkit-rs:0.0.1 .

# Package the Helm chart into dist/.
chart:
    @helm package charts/ibkit-gateway --destination dist

# Everything CI runs.
ci: fmt test-all tui-check chart-check

# Rewrite crates/ibkit-gateway/openapi.json from the Rust types, and the SDK types from it.
openapi:
    UPDATE_OPENAPI=1 cargo test -q -p ibkit-cli --test control_api the_checked_in_openapi_document_is_current
    @uv run --no-project python scripts/gen_gateway_types.py

# Fault-injection soak test against a paper account (see scripts/soak.sh).
soak *args:
    cargo build --release --locked -p ibkit-cli
    scripts/soak.sh {{args}}
