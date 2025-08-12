//! Subblock executor.
//!
//! This is a standalone program that can be used to execute a subblock, and optionally dump the
//! elf/stdin pairs to a directory.

use alloy_provider::ReqwestProvider;
use clap::Parser;
use pico_sdk::{client::DefaultProverClient, init_logger, load_elf, HashableKey};
use rsp_client_executor::{io::SubblockHostOutput, ChainVariant};
use rsp_host_executor::HostExecutor;
use std::{path::PathBuf, time::Instant};
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
    /// Whether to execute the subblock and aggregation programs in the SP1 zkVM.
    ///
    /// Note: does not generate a proof.
    #[clap(long)]
    execute: bool,
    #[clap(long)]
    prove: bool,
    /// Where to dump the elf and stdin for the subblock and aggregation programs.
    #[clap(long)]
    dump_dir: Option<PathBuf>,
    /// Optional path to the directory containing cached client input. A new cache file will be
    /// created from RPC data if it doesn't already exist.
    #[clap(long)]
    cache_dir: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    let t_start = Instant::now();
    let t_client_input = Instant::now();
    // Intialize the environment variables.
    dotenv::dotenv().ok();

    // Initialize the logger.
    // tracing_subscriber::registry()
    //     .with(fmt::layer().compact().with_target(false).with_file(false).
    // with_thread_names(false))     .with(
    //         EnvFilter::try_from_default_env()
    //             .unwrap_or_else(|_| EnvFilter::new("info"))
    //             .add_directive("pico_sdk=debug".parse().unwrap())
    //             .add_directive("pico_vm=info".parse().unwrap())
    //             .add_directive("p3_keccak_air=off".parse().unwrap())
    //             .add_directive("p3_fri=off".parse().unwrap())
    //             .add_directive("p3_dft=off".parse().unwrap())
    //             .add_directive("p3_matrix=off".parse().unwrap())
    //             .add_directive("p3_merkle_tree=off".parse().unwrap())
    //             .add_directive("p3_field=off".parse().unwrap())
    //             .add_directive("p3_challenger=off".parse().unwrap()),
    //     )
    //     .init();
    init_logger();

    // Parse the command line arguments.
    let args = HostArgs::parse();

    let provider_config = args.provider.clone().into_provider().await?;

    let cache_data = try_load_input_from_cache(
        args.cache_dir.as_ref(),
        provider_config.chain_id,
        args.block_number,
    )?;

    let client_input = match (cache_data, provider_config.rpc_url) {
        (Some(cache_data), _) => cache_data,
        (None, Some(rpc_url)) => {
            // Cache not found but we have RPC
            // Setup the provider.
            let provider = ReqwestProvider::new_http(rpc_url);

            // Setup the host executor.
            let host_executor = HostExecutor::new(provider);

            // Execute the host.
            let t_prepare_sb_stdin = Instant::now();
            let cache_data = host_executor
                .execute_subblock(args.block_number, ChainVariant::Ethereum)
                .await
                .expect("failed to execute host");

            println!("TIMER_ALL preprocess subblocks stdin: {:.3?}", t_prepare_sb_stdin.elapsed());

            let t_write_to_cache = Instant::now();

            if let Some(ref cache_dir) = args.cache_dir {
                let input_folder = cache_dir.join(format!("input/{}", provider_config.chain_id));
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
        (None, None) => {
            eyre::bail!("cache not found and RPC URL not provided")
        }
    };
    println!("TIMER_ALL t_client_input: {:.3?}", t_client_input.elapsed());

    let t_post_client_input = Instant::now();
    let t_setup_client = Instant::now();
    // Generate the proof.
    let subblock_elf = load_elf("./bin/client-eth-subblock/pico-elf/riscv32im-pico-zkvm-elf");
    let subblock_client = DefaultProverClient::new(&subblock_elf);
    let agg_elf = load_elf("./bin/client-eth-agg/pico-elf/riscv32im-pico-zkvm-elf");
    let agg_client = DefaultProverClient::new(&agg_elf);

    println!("TIMER_ALL t_setup_client: {:.3?}", t_setup_client.elapsed());

    schedule_subblock_execution(
        subblock_client,
        args.block_number,
        agg_client,
        client_input,
        args.execute,
        args.prove,
        args.dump_dir,
    )
    .await?;

    println!("TIMER_ALL t_post_client_input: {:.3?}", t_post_client_input.elapsed());

    println!("TIMER_ALL entire main fn: {:.3?}", t_start.elapsed());

    Ok(())
}

async fn schedule_subblock_execution(
    subblock_client: DefaultProverClient,
    block_number: u64,
    agg_client: DefaultProverClient,
    inputs: SubblockHostOutput,
    execute: bool,
    prove: bool,
    dump_dir: Option<PathBuf>,
) -> eyre::Result<()> {
    let t_dump = Instant::now();
    // let (subblock_elf, subblock_vk) = (subblock_pk.elf, subblock_pk.vk);
    // let agg_elf = agg_pk.elf;
    //
    // let dump_dir = dump_dir.map(|d| d.join(format!("{}", block_number)));
    //
    // if let Some(dump_dir) = dump_dir.as_ref() {
    //     std::fs::create_dir_all(dump_dir)?;
    //     std::fs::write(dump_dir.join("subblock_elf.bin"), &subblock_elf)?;
    //     std::fs::write(dump_dir.join("subblock_vk.bin"), bincode::serialize(&subblock_vk)?)?;
    //     std::fs::write(dump_dir.join("agg_elf.bin"), &agg_elf)?;
    // }

    // let client =
    //     tokio::task::spawn_blocking(|| ProverClient::builder().cpu().build()).await.unwrap();
    // if let Some(dump_dir) = dump_dir.as_ref() {
    //     let stdin_path = dump_dir.join("agg_stdin.bin");
    //     std::fs::write(stdin_path, bincode::serialize(&aggregation_stdin)?)?;
    // }

    println!(
        "TIMER aggregator stdin & dump_dir in schedule_subblock_execution: {:.3?}",
        t_dump.elapsed()
    );

    let t = Instant::now();
    let mut riscv_proofs = Vec::new();
    let mut combine_proofs = Vec::new();
    let subblock_vk = subblock_client.riscv_vk().clone();

    for i in 0..inputs.subblock_inputs.len() {
        println!("----------------------Subblock {}-----------------------", i);
        let input = &inputs.subblock_inputs[i];
        let parent_state = &inputs.subblock_parent_states[i];

        let mut stdin_builder = subblock_client.new_stdin_builder();
        stdin_builder.write(input);
        stdin_builder.write_slice(parent_state);

        // Save the elf/stdin pair to the dump directory.
        if let Some(dump_dir) = dump_dir.as_ref() {
            let stdin_dir_path = dump_dir.join("subblock_stdins");
            std::fs::create_dir_all(&stdin_dir_path)?;
            let stdin_path = stdin_dir_path.join(format!("{}.bin", i));
            std::fs::write(stdin_path, bincode::serialize(&stdin_builder)?)?;
        }

        // TODO: use prove flag
        // Generate proof
        let start = Instant::now();
        let (riscv_proof, combine_proof) =
            subblock_client.prove_combine(stdin_builder.clone()).expect("Failed to generate proof");
        let elapsed = start.elapsed().as_secs_f64();

        tracing::info!("Subblock {}: prove duration: {:?}", i, elapsed,);

        riscv_proofs.push(riscv_proof);
        combine_proofs.push(combine_proof);

        if execute {
            let start = Instant::now();

            let (cycles, _pv_stream) = subblock_client.emulate(stdin_builder.clone());

            let elapsed = start.elapsed().as_secs_f64();
            let subblock_instruction_count = cycles;
            let hz = subblock_instruction_count as f64 / elapsed;
            let mhz = hz / 1_000_000.0;

            tracing::info!(
                "Subblock {}: {} instructions in {:.3} s → {:.3} MHz",
                i,
                subblock_instruction_count,
                elapsed,
                mhz
            );
        }
    }
    println!("----------------------Aggregator-----------------------");

    let t_agg_stdin = Instant::now();
    // let aggregation_stdin = to_aggregation_stdin(inputs.clone(), &subblock_client.riscv_vk());

    let mut stdin_builder = agg_client.new_stdin_builder();

    let subblock_host_output = inputs;
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

    stdin_builder.write::<Vec<Vec<u8>>>(&public_values);
    stdin_builder.write::<[u32; 8]>(&subblock_client.riscv_vk().hash_u32());
    stdin_builder.write(&subblock_host_output.agg_input);
    stdin_builder.write(&subblock_host_output.agg_input.parent_header().state_root);
    assert_eq!(riscv_proofs.len(), combine_proofs.len());
    for i in 0..riscv_proofs.len() {
        stdin_builder.write_pico_proof(combine_proofs[i].clone(), subblock_vk.clone());
    }
    println!("TIMER aggregator stdin: {:?}", t_agg_stdin.elapsed());

    let start = Instant::now();
    // Execute the aggregation program with deferred proof verification off, since we don't have the
    // proof yet.
    let (agg_riscv_proof, agg_combine_proof) =
        agg_client.prove_combine(stdin_builder.clone()).expect("Failed to generate proof");
    let elapsed = start.elapsed().as_secs_f64();

    tracing::info!("Aggregator: prove duration: {:?}", elapsed,);

    if execute {
        let start = Instant::now();
        // Execute the aggregation program with deferred proof verification off, since we don't have
        // the proof yet.
        let (cycles, _pv_stream) = agg_client.emulate(stdin_builder);
        let elapsed = start.elapsed().as_secs_f64();

        let agg_instruction_count = cycles;
        let hz = agg_instruction_count as f64 / elapsed;
        let mhz = hz / 1_000_000.0;

        tracing::info!(
            "Aggregator: {} cycles/instructions in {:.3} s → {:.3} MHz",
            agg_instruction_count,
            elapsed,
            mhz
        );
        // tracing::info!("Aggregation program instruction count: {}", agg_instruction_count);
    }
    println!("TIMER execute subblocks and aggregator: {:?}", t.elapsed());

    Ok(())
}
//
// /// Constructs the aggregation stdin, minus the subblock proofs.
// pub fn to_aggregation_stdin(
//     subblock_host_output: SubblockHostOutput,
//     subblock_client: &DefaultProverClient,
//     agg_client: &DefaultProverClient,
// ) -> EmulatorStdinBuilder<Vec<u8>> {
//     let mut stdin_builder = agg_client.new_stdin_builder();
//
//     assert_eq!(
//         subblock_host_output.subblock_inputs.len(),
//         subblock_host_output.subblock_outputs.len()
//     );
//     let mut public_values = Vec::new();
//     for i in 0..subblock_host_output.subblock_inputs.len() {
//         let mut current_public_values = Vec::new();
//         let input = &subblock_host_output.subblock_inputs[i];
//         bincode::serialize_into(&mut current_public_values, input).unwrap();
//         bincode::serialize_into(
//             &mut current_public_values,
//             &subblock_host_output.subblock_outputs[i],
//         )
//         .unwrap();
//         public_values.push(current_public_values);
//     }
//
//     tracing::info!(
//         "Public values size in bytes: {}",
//         public_values.iter().map(|v| v.len()).sum::<usize>()
//     );
//
//     // // Deserialize the parent state and compute the root.
//     // let mut aligned_vec = AlignedVec::<16>::new();
//     // let mut reader = Cursor::new(&subblock_host_output.agg_parent_state);
//     // aligned_vec.extend_from_reader(&mut reader).unwrap();
//     // let parent_state =
//     //     rkyv::from_bytes::<EthereumState, rkyv::rancor::BoxedError>(&aligned_vec).unwrap();
//     // let parent_state_root = parent_state.state_root();
//
//     stdin_builder.write::<Vec<Vec<u8>>>(&public_values);
//     stdin_builder.write::<[u32; 8]>(&subblock_client.riscv_vk().hash_u32());
//     stdin_builder.write(&subblock_host_output.agg_input);
//     stdin_builder.write(&subblock_host_output.agg_input.parent_header().state_root);
//     stdin_builder
// }

fn try_load_input_from_cache(
    cache_dir: Option<&PathBuf>,
    chain_id: u64,
    block_number: u64,
) -> eyre::Result<Option<SubblockHostOutput>> {
    Ok(if let Some(cache_dir) = cache_dir {
        let cache_path =
            cache_dir.join(format!("subblock-input/{}/{}.bin", chain_id, block_number));

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
