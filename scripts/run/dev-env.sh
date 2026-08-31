# Development environment variables for running the Rivet engine locally.
# Source this file before running the engine: source scripts/run/dev-env.sh
#
# NOTE: When modifying these env vars, also update `engine_env` in
# rivetkit-rust/packages/engine-process/src/lib.rs to keep them in sync.

# Reduce backoff for actor recovery (in milliseconds)
export RIVET__PEGBOARD__RETRY_RESET_DURATION="100"
export RIVET__PEGBOARD__BASE_RETRY_TIMEOUT="100"
export RIVET__PEGBOARD__RESCHEDULE_BACKOFF_MAX_EXPONENT="1"

# Reduce thresholds for faster development iteration (in milliseconds)
# Legacy runner paths still exist in older Engine builds, while RivetKit uses
# the Envoy path. Keep both tuned for local development.
export RIVET__PEGBOARD__RUNNER_ELIGIBLE_THRESHOLD="5000"
export RIVET__PEGBOARD__RUNNER_LOST_THRESHOLD="7000"
export RIVET__PEGBOARD__ENVOY_ELIGIBLE_THRESHOLD="5000"
export RIVET__PEGBOARD__ENVOY_LOST_THRESHOLD="7000"

# Allow faster metadata polling for hot-reload in development (in milliseconds)
export RIVET__PEGBOARD__MIN_METADATA_POLL_INTERVAL="1000"

# Reduce shutdown durations for faster development iteration (in seconds)
export RIVET__RUNTIME__WORKER_SHUTDOWN_DURATION="1"
export RIVET__RUNTIME__GUARD_SHUTDOWN_DURATION="1"
export RIVET__RUNTIME__FORCE_SHUTDOWN_DURATION="2"
