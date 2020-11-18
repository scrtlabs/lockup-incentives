use cosmwasm_std::{Coin, HumanAddr, Uint128};
use std::collections::HashMap;

pub type Lockups = HashMap<HumanAddr, Lockup>;

#[derive(Serialize, Deserialize, Eq, PartialEq, Debug)]
pub struct Lockup {
    locked: Vec<Coin>,
    pending_rewards: Uint128,
}

pub type SupportedTokens = Vec<Token>;

pub struct Token {
    denom: String,
    weight: u64,
}

pub struct Config {
    pub admin: HumanAddr,
}
