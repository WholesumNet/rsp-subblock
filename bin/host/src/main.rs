//! Subblock executor.
//!
//! This is a standalone program that can be used to execute a subblock, and optionally dump the
//! elf/stdin pairs to a directory.

#![allow(deprecated)]

use alloy_provider::ReqwestProvider;
use clap::Parser;
use sp1_sdk::{
    include_elf, HashableKey, Prover, ProverClient, CpuProver, SP1Stdin, ProvingKey, SP1VerifyingKey,
};
use rsp_client_executor::{
    io::{AggregationInput, SubblockHostOutput},
    ChainVariant,
};
use rsp_host_executor::HostExecutor;
use std::{
    env,
    fs::File,
    io::Write,
    path::{Path, PathBuf},
    time::Instant,
};
use tracing_subscriber::{
    filter::EnvFilter, fmt, prelude::__tracing_subscriber_SubscriberExt, util::SubscriberInitExt,
};

mod cli;
use cli::ProviderArgs;

/// The arguments for the subblock executable.
#[derive(Debug, Clone, Parser)]
struct HostArgs {
    /// The block number of the block to execute.
    #[clap(long)]
    block_number: u64,
    #[clap(flatten)]
    provider: ProviderArgs,

    #[clap(long)]
    execute: bool,
    #[clap(long)]
    prove: bool,
    #[clap(long)]
    execution_witness: bool,

    /// Where to dump the elf and stdin for the subblock and aggregation programs.
    #[clap(long)]
    dump_dir: Option<PathBuf>,

    /// Optional path to the directory containing cached client input. A new cache file will be
    /// created from RPC data if it doesn't already exist.
    #[clap(long)]
    cache_dir: Option<PathBuf>,
}

#[allow(unused)]
fn resolve_dump_dir(dump_dir: Option<&PathBuf>, block_number: u64) -> PathBuf {
    let gas_segment = match env::var("SUBBLOCK_GAS_LIMIT").ok().and_then(|s| s.parse::<u64>().ok())
    {
        Some(g) => format!("gas{}", g),
        None => "gasUNSET".to_string(),
    };

    let base = dump_dir.cloned().unwrap_or_else(|| PathBuf::from("."));

    base.join(format!("block{}", block_number)).join(gas_segment)
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    dotenv::dotenv().ok();    
    // Initialize the logger.
    tracing_subscriber::registry().with(fmt::layer()).with(EnvFilter::from_default_env()).init();  

    let t_start = Instant::now();
    let t_client_input = Instant::now();

    // Parse the command line arguments.
    let args = HostArgs::parse();

    let provider_config = args.provider.clone().into_provider().await?;

    let cache_data = try_load_input_from_cache(
        args.cache_dir.as_ref(),
        provider_config.chain_id,
        args.block_number,
    )?;

    let client_input =
        match (cache_data, provider_config.basic_rpc_url, provider_config.debug_rpc_url) {
            (Some(cache_data), _, _) => cache_data,
            (None, Some(basic_rpc_url), Some(debug_rpc_url)) => {
                // Cache not found but we have RPC
                // Setup the provider.
                let basic_provider = ReqwestProvider::new_http(basic_rpc_url);
                let debug_provider = ReqwestProvider::new_http(debug_rpc_url);

                // Setup the host executor.
                let host_executor = HostExecutor::new(basic_provider, debug_provider);

                // Execute the host.
                let t_prepare_sb_stdin = Instant::now();
                let cache_data = host_executor
                    .execute_subblock(
                        args.execution_witness,
                        args.block_number,
                        ChainVariant::Ethereum,
                        args.dump_dir.clone(),
                    )
                    .await
                    .expect("failed to execute host");

                println!(
                    "TIMER_ALL preprocess subblocks stdin: {:.3?}",
                    t_prepare_sb_stdin.elapsed()
                );

                let t_write_to_cache = Instant::now();

                if let Some(ref cache_dir) = args.cache_dir {
                    let input_folder =
                        cache_dir.join(format!("input/{}", provider_config.chain_id));
                    if !input_folder.exists() {
                        std::fs::create_dir_all(&input_folder)?;
                    }

                    let input_path = input_folder.join(format!("{}.bin", args.block_number));
                    let mut cache_file = std::fs::File::create(input_path)?;

                    bincode::serialize_into(&mut cache_file, &cache_data)?;
                }

                println!("write_to_cache time: {:?}", t_write_to_cache.elapsed());

                cache_data
            }
            _ => {
                eyre::bail!("cache not found and RPC URL not provided")
            }
        };
    println!("TIMER_ALL t_client_input: {:.3?}", t_client_input.elapsed());

    let t_post_client_input = Instant::now();
    let t_setup_client = Instant::now();
    // Generate the proof.
    let client = ProverClient::builder().cpu().build().await;    

    println!("TIMER_ALL t_setup_client: {:.3?}", t_setup_client.elapsed());

    schedule_subblock_execution(
        client,
        args.block_number,
        client_input,
        args.execute
    )
    .await?;

    println!("TIMER_ALL t_post_client_input: {:.3?}", t_post_client_input.elapsed());

    println!("TIMER_ALL entire main fn: {:.3?}", t_start.elapsed());

    Ok(())
}

async fn schedule_subblock_execution(
    client: CpuProver,
    block_number: u64,
    inputs: SubblockHostOutput,
    execute: bool,
) -> eyre::Result<()> {
    let block_dir = format!("./artifacts/{}", block_number);
    let artifacts_path = Path::new(&block_dir);
    std::fs::create_dir_all(&artifacts_path)?;    

    // Setup the proving key and verification key.
    let subblock_pk = client.setup(include_elf!("rsp-client-eth-subblock")).await?;

    let agg_pk = client.setup(include_elf!("rsp-client-eth-agg")).await?;
    
    let aggregation_stdin = to_aggregation_stdin(inputs.clone(), subblock_pk.verifying_key());
    std::fs::write(
        artifacts_path.join("agg-stdin.bin"),
        bincode::serialize(&aggregation_stdin)?
    )?;

    let subblock_stdins_path = artifacts_path.join("subblock-stdins");
    std::fs::create_dir_all(&subblock_stdins_path)?;
    for i in 0..inputs.subblock_inputs.len() {
        println!("----------------------Subblock {}-----------------------", i);
        let input = &inputs.subblock_inputs[i];
        let parent_state = &inputs.subblock_parent_states[i];

        let mut stdin = SP1Stdin::new();
        stdin.write(input);
        stdin.write_vec(parent_state.clone());
        
        // Save the elf/stdin pair to the dump directory.
        std::fs::write(
            subblock_stdins_path.join(format!("{}.bin", i)),
            bincode::serialize(&stdin)?
        )?;

        if execute {
            let start = Instant::now();            
            let (_public_values, report) = client.execute(subblock_pk.elf().clone(), stdin).await.unwrap();
            let elapsed = start.elapsed().as_secs_f64();

            let subblock_instruction_count = report.total_instruction_count();

            let hz = subblock_instruction_count as f64 / elapsed;
            let mhz = hz / 1_000_000.0;
            println!(
                "Subblock {}: {} instructions in {:.3} s → {:.3} MHz",
                i,
                subblock_instruction_count,
                elapsed,
                mhz
            );
        }
    }
    
    println!("----------------------Aggregator-----------------------");
    if execute {
        let start = Instant::now();
        // Execute the aggregation program with deferred proof verification off, since we don't have
        // the proof yet.
        let (_public_values, report) = client
            .execute(agg_pk.elf().clone(), aggregation_stdin)
            .deferred_proof_verification(false)
            .await
            .unwrap();
        let elapsed = start.elapsed().as_secs_f64();
        
        let agg_instruction_count = report.total_instruction_count();
        let hz = agg_instruction_count as f64 / elapsed;
        let mhz = hz / 1_000_000.0;

        println!(
            "Aggregator: {} instructions in {:.3} s → {:.3} MHz",
            agg_instruction_count,
            elapsed,
            mhz
        );
    }

    Ok(())
}
//
/// Constructs the aggregation stdin, minus the subblock proofs.
pub fn to_aggregation_stdin(
    subblock_host_output: SubblockHostOutput,
    subblock_vk: &SP1VerifyingKey,
) -> SP1Stdin {
    let mut stdin = SP1Stdin::new();

    assert_eq!(
        subblock_host_output.subblock_inputs.len(),
        subblock_host_output.subblock_outputs.len()
    );
    let mut public_values = Vec::new();
    for i in 0..subblock_host_output.subblock_inputs.len() {
        let mut current_public_values = Vec::new();
        let input = &subblock_host_output.subblock_inputs[i];
        bincode::serialize_into(&mut current_public_values, input).unwrap();
        bincode::serialize_into(
            &mut current_public_values,
            &subblock_host_output.subblock_outputs[i],
        )
        .unwrap();
        public_values.push(current_public_values);
    }

    tracing::info!(
        "Public values size in bytes: {}",
        public_values.iter().map(|v| v.len()).sum::<usize>()
    );

    // // Deserialize the parent state and compute the root.
    // let mut aligned_vec = AlignedVec::<16>::new();
    // let mut reader = Cursor::new(&subblock_host_output.agg_parent_state);
    // aligned_vec.extend_from_reader(&mut reader).unwrap();
    // let parent_state =
    //     rkyv::from_bytes::<EthereumState, rkyv::rancor::BoxedError>(&aligned_vec).unwrap();
    // let parent_state_root = parent_state.state_root();

    stdin.write::<Vec<Vec<u8>>>(&public_values);
    stdin.write::<[u32; 8]>(&subblock_vk.hash_u32());
    stdin.write(&subblock_host_output.agg_input);
    stdin.write(&subblock_host_output.agg_input.parent_header().state_root);
    stdin
}

#[allow(unused)]
fn dump_agg_stdin_to_files(
    public_values: &Vec<Vec<u8>>,
    vk_digest: &[u32; 8],
    agg_input: &AggregationInput,
    out_dir: &Path,
) -> std::io::Result<()> {
    // ensure directory exists
    std::fs::create_dir_all(out_dir)?;

    // 1. public_values
    let bytes = bincode::serialize(public_values).unwrap();
    File::create(out_dir.join("public_values.bin"))?.write_all(&bytes)?;

    // 2. vk_digest
    let bytes = bincode::serialize(vk_digest).unwrap();
    File::create(out_dir.join("vk_digest.bin"))?.write_all(&bytes)?;

    // 3. agg_input
    let bytes = bincode::serialize(agg_input).unwrap();
    File::create(out_dir.join("agg_input.bin"))?.write_all(&bytes)?;

    // 4. state_root
    let bytes = bincode::serialize(&agg_input.parent_header().state_root).unwrap();
    File::create(out_dir.join("state_root.bin"))?.write_all(&bytes)?;

    Ok(())
}

fn try_load_input_from_cache(
    cache_dir: Option<&PathBuf>,
    chain_id: u64,
    block_number: u64,
) -> eyre::Result<Option<SubblockHostOutput>> {
    Ok(if let Some(cache_dir) = cache_dir {
        let cache_path = cache_dir.join(format!("input/{}/{}.bin", chain_id, block_number));

        if cache_path.exists() {
            // Try to open and deserialize the cache file, delete it if there's an error
            match (|| -> eyre::Result<SubblockHostOutput> {
                let mut cache_file = std::fs::File::open(&cache_path)?;
                let cache_data: SubblockHostOutput = bincode::deserialize_from(&mut cache_file)?;
                Ok(cache_data)
            })() {
                Ok(cache_data) => Some(cache_data),
                Err(err) => {
                    tracing::warn!("Failed to load cache file {}: {}", cache_path.display(), err);
                    // Delete the invalid cache file
                    if let Err(delete_err) = std::fs::remove_file(&cache_path) {
                        tracing::warn!("Failed to delete invalid cache file: {}", delete_err);
                    } else {
                        tracing::info!("Deleted invalid cache file: {}", cache_path.display());
                    }
                    None
                }
            }
        } else {
            None
        }
    } else {
        None
    })
}
