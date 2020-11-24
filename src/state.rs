use cosmwasm_std::{Coin, HumanAddr, Uint128};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub type Lockups = HashMap<HumanAddr, Lockup>;

#[derive(Serialize, Deserialize, Debug)]
pub struct Lockup {
    pub locked: u128,
    pub pending_rewards: u128,
}

#[derive(Serialize, Deserialize, Eq, PartialEq, Debug, Clone, JsonSchema)]
pub struct Snip20 {
    pub address: HumanAddr,
    pub contract_hash: String,
}

#[derive(Serialize, Deserialize, Eq, PartialEq, Debug, Clone)]
pub struct Config {
    pub admin: HumanAddr,
    pub reward_token: Snip20,
    pub incentivized: Snip20,
    pub pool_claim_height: u64,
    pub viewing_key: String,
    pub prng_seed: Vec<u8>,
}
