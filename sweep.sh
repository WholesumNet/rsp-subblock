#!/usr/bin/env bash
set -euo pipefail

# --------- Configuration ---------
export RUST_BACKTRACE=full
export CHUNK_SIZE=4194304
export CHUNK_BATCH_SIZE=16
export SPLIT_THRESHOLD=32768
export RUSTFLAGS="-C target-cpu=native -C target-feature=+avx512f,+avx512ifma,+avx512vl"
export VK_VERIFICATION=true

BLOCK_NUMBERS=(
  23043570
  23043243
  23042521
  23042783
  23043752
  23044137
  23043427
  23042831
  23044286
  23044323
  23043964
  23044081
  23043757
  23044401
  23043439
  23043259
  23043464
  23044446
  23044316
  23044002
  23042747
  23043634
)

CHAIN_ID=1
RPC_URL=
# GAS_LIMITS=(16000000 8000000 1000000)
# GAS_LIMITS=(1000000 2000000 4000000 8000000)
GAS_LIMITS=(8000000)
DUMP_DIR=./dump_dir
CACHE_DIR=./cache_dir
LOG_DIR=./logs
# RUST_LOG_LEVEL="info,pico_sdk=debug,pico_vm=debug,rsp_host_executor=info,rsp_client_executor=info,alloy_provider=warn"
# RUST_LOG_LEVEL=debug
RUST_LOG_LEVEL=info
# RUST_LOG="info,pico_sdk=debug,pico_vm=info,rsp_host_executor=info,rsp_client_executor=info,alloy_provider=warn"

# --------------------------------

mkdir -p "$DUMP_DIR" "$CACHE_DIR" "$LOG_DIR"

for BLOCK_NUMBER in "${BLOCK_NUMBERS[@]}"; do
  for GAS in "${GAS_LIMITS[@]}"; do
    export SUBBLOCK_GAS_LIMIT="$GAS"

    ts=$(date +%Y%m%d-%H%M%S)
    log_file="$LOG_DIR/run_block${BLOCK_NUMBER}_gas${GAS}_${ts}.log"

    echo "[$(date '+%F %T')] RUN SUBBLOCK_GAS_LIMIT=${GAS} -> $log_file"

    RUST_LOG="$RUST_LOG_LEVEL" cargo run -r --bin subblock -- \
      --block-number "$BLOCK_NUMBER" \
      --chain-id "$CHAIN_ID" \
      --rpc-url "$RPC_URL" \
      --dump-dir "$DUMP_DIR" \
      --cache-dir "$CACHE_DIR" \
      --execute \
      2>&1 | tee "$log_file"
  done
done
