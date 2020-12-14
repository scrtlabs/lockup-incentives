use cosmwasm_std::HumanAddr;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct UserInfo {
    pub locked: u128,
    pub debt: u128,
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
    pub inc_token: Snip20,
    pub pool_claim_block: u64,
    pub deadline: u64,
    pub viewing_key: String,
    pub prng_seed: Vec<u8>,
    pub is_stopped: bool,
}

/// RewardPool is a struct that keeps track of rewards and lockups
///
/// `pending_rewards` - Rewards left to distribute.
/// `inc_token_supply` - Total supply of the incentivized token that is locked in the contract.
///  This number is scaled down by `constants::INC_TOKEN_SCALE`. Keeping track of it so external query will not
///  be necessary every time a user locks/redeems tokens.
/// `last_reward_block` - Last block in which rewards got updated.
/// `acc_reward_per_share` - Accumulated rewards per share. This number is scaled up by `constants::REWARD_SCALE`
///  and shares scaled the same way as `inc_token_supply`.
#[derive(Serialize, Deserialize, Eq, PartialEq, Debug, Clone)]
pub struct RewardPool {
    pub pending_rewards: u128,
    pub inc_token_supply: u128,
    pub last_reward_block: u64,
    pub acc_reward_per_share: u128,
}
