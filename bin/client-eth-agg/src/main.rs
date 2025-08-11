#![no_main]
pico_sdk::entrypoint!(main);

use reth_primitives::B256;
use rsp_client_executor::{io::AggregationInput, ClientExecutor, EthereumVariant};

pub fn main() {
    // Read the input.
    println!("cycle-tracker-start: deserialize");
    // Read the public values, vkey, and aggregation input.
    let public_values = pico_sdk::io::read_as::<Vec<Vec<u8>>>();
    let vkey = pico_sdk::io::read_as::<[u32; 8]>();
    println!("cycle-tracker-start: deserialize aggregation input");
    let aggregation_input = pico_sdk::io::read_as::<AggregationInput>();
    println!("cycle-tracker-end: deserialize aggregation input");

    let parent_state_root = pico_sdk::io::read_as::<B256>();
    pico_sdk::io::commit(&parent_state_root);
    pico_sdk::io::commit(&aggregation_input.current_block);
    println!("cycle-tracker-end: deserialize");

    let client = ClientExecutor;

    let header = client
        .execute_aggregation::<EthereumVariant>(
            public_values,
            vkey,
            aggregation_input,
            parent_state_root,
        )
        .expect("failed to execute aggregation");

    let hash = header.hash_slow();

    pico_sdk::io::commit(&hash);
}
