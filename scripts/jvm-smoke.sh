#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
: "${JAVA_HOME:?Set JAVA_HOME to a Java 17 or newer JDK and run this script again.}"
cargo build -p ibkit-cli
case "$(uname -s)" in
  Darwin) IBKIT_AGENT_LIBRARY="$PWD/target/debug/libibkit_gateway_agent.dylib" ;;
  Linux) IBKIT_AGENT_LIBRARY="$PWD/target/debug/libibkit_gateway_agent.so" ;;
  *) echo 'Run the JVM checks on Linux or macOS.' >&2; exit 1 ;;
esac
export IBKIT_AGENT_LIBRARY
cargo nextest run --locked --profile full -p ibkit-cli --test jvm --run-ignored all
