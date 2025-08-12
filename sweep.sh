#!/usr/bin/env bash
set -euo pipefail

# --------- Configuration ---------
export CHUNK_SIZE=4194304
BLOCK_NUMBER=20528658
CHAIN_ID=1
# GAS_LIMITS=(16000000 8000000 1000000)
GAS_LIMITS=(1000000 2000000 4000000 8000000 )
DUMP_DIR=./dump_dir
CACHE_DIR=./cache_dir
LOG_DIR=./logs
RUST_LOG_LEVEL=info                        
# --------------------------------

mkdir -p "$DUMP_DIR" "$CACHE_DIR" "$LOG_DIR"


for GAS in "${GAS_LIMITS[@]}"; do
  export SUBBLOCK_GAS_LIMIT="$GAS"

  ts=$(date +%Y%m%d-%H%M%S)
  log_file="$LOG_DIR/run_block${BLOCK_NUMBER}_gas${GAS}_${ts}.log"

  echo "[$(date '+%F %T')] RUN SUBBLOCK_GAS_LIMIT=${GAS} -> $log_file"

  RUST_LOG="$RUST_LOG_LEVEL" cargo run --release --bin subblock -- \
    --block-number "$BLOCK_NUMBER" \
    --chain-id "$CHAIN_ID" \
    --execute \
    2>&1 | tee "$log_file"
    # --dump-dir "$DUMP_DIR" \
    # --cache-dir "$CACHE_DIR" \
  
done
