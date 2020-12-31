use crate::state::Snip20;
use crate::viewing_key::ViewingKey;
use cosmwasm_std::{Binary, HumanAddr, Uint128};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InitMsg {
    pub reward_token: Snip20,
    pub inc_token: Snip20,
    pub deadline: u64,
    pub pool_claim_block: u64,
    pub viewing_key: String,
    pub prng_seed: Binary,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HandleMsg {
    Redeem {
        amount: Option<Uint128>,
    },
    CreateViewingKey {
        entropy: String,
        padding: Option<String>,
    },
    SetViewingKey {
        key: String,
        padding: Option<String>,
    },
    EmergencyRedeem {},

    // Registered commands
    Receive {
        sender: HumanAddr,
        from: HumanAddr,
        amount: Uint128,
        msg: Binary,
    },

    // Admin commands
    SetDeadline {
        block: u64,
    },
    ClaimRewardPool {
        to: Option<HumanAddr>,
    },
    StopContract {},
    ResumeContract {},
    ChangeAdmin {
        address: HumanAddr,
    },
}

#[derive(Serialize, Deserialize, JsonSchema, Debug)]
#[serde(rename_all = "snake_case")]
pub enum HandleAnswer {
    Redeem { status: ResponseStatus },
    CreateViewingKey { key: ViewingKey },
    SetViewingKey { status: ResponseStatus },
    StopContract { status: ResponseStatus },
    ResumeContract { status: ResponseStatus },
    ChangeAdmin { status: ResponseStatus },
    SetDeadline { status: ResponseStatus },
    ClaimRewardPool { status: ResponseStatus },
    EmergencyRedeem { status: ResponseStatus },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReceiveMsg {
    Deposit {},
    DepositRewards {},
}

#[derive(Serialize, Deserialize, JsonSchema, Debug)]
#[serde(rename_all = "snake_case")]
pub enum ReceiveAnswer {
    Deposit { status: ResponseStatus },
    DepositRewards { status: ResponseStatus },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    TokenInfo {},
    ClaimBlock {},
    ContractStatus {},
    RewardToken {},
    IncentivizedToken {},
    EndHeight {},
    RewardPoolBalance {},

    // Authenticated
    Rewards {
        address: HumanAddr,
        height: u64,
        key: String,
    },
    Deposit {
        address: HumanAddr,
        key: String,
    },
}

impl QueryMsg {
    pub fn get_validation_params(&self) -> (&HumanAddr, ViewingKey) {
        match self {
            QueryMsg::Rewards { address, key, .. } => (address, ViewingKey(key.clone())),
            QueryMsg::Deposit { address, key } => (address, ViewingKey(key.clone())),
            _ => panic!("This should never happen"),
        }
    }
}

#[derive(Serialize, Deserialize, JsonSchema, Debug)]
#[serde(rename_all = "snake_case")]
pub enum QueryAnswer {
    TokenInfo {
        name: String,
        symbol: String,
        decimals: u8,
        total_supply: Option<Uint128>,
    },
    Rewards {
        rewards: Uint128,
    },
    Deposit {
        deposit: Uint128,
    },
    ClaimBlock {
        height: u64,
    },
    ContractStatus {
        is_stopped: bool,
    },
    RewardToken {
        token: Snip20,
    },
    IncentivizedToken {
        token: Snip20,
    },
    EndHeight {
        height: u64,
    },
    RewardPoolBalance {
        balance: Uint128,
    },

    QueryError {
        msg: String,
    },
}

#[derive(Serialize, Deserialize, Clone, PartialEq, JsonSchema, Debug)]
#[serde(rename_all = "snake_case")]
pub enum ResponseStatus {
    Success,
    Failure,
}
