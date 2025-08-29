use reth_chainspec::{BaseFeeParams, BaseFeeParamsKind, Chain, ChainSpec, EthereumHardfork};

pub fn mainnet() -> ChainSpec {
    ChainSpec {
        chain: Chain::mainnet(),
        genesis: Default::default(),
        genesis_header: Default::default(),
        paris_block_and_final_difficulty: Default::default(),
        hardforks: EthereumHardfork::mainnet().into(),
        deposit_contract: Default::default(),
        base_fee_params: BaseFeeParamsKind::Constant(BaseFeeParams::ethereum()),
        prune_delete_limit: 20000,
        blob_params: Default::default(),
    }
}
