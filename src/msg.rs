use crate::state::Snip20;
use crate::viewing_key::ViewingKey;
use cosmwasm_std::{Binary, HumanAddr, Uint128};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InitMsg {
    pub reward_token: Snip20,
    pub incentivized: Snip20,
    pub end_by_height: u64,
    pub pool_claim_block: u64,
    pub viewing_key: String,
    pub prng_seed: Binary,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HandleMsg {
    LockTokens {},
    AddToRewardPool {},
    Redeem {
        amount: Option<Uint128>,
    },
    WithdrawRewards {},
    CreateViewingKey {
        entropy: String,
        padding: Option<String>,
    },
    SetViewingKey {
        key: String,
        padding: Option<String>,
    },

    // Registered commands
    Receive {
        sender: HumanAddr,
        from: HumanAddr,
        amount: Uint128,
        msg: Binary,
    },

    // Admin commands
    UpdateIncentivizedToken {
        new_token: Snip20,
    },
    UpdateRewardToken {
        new_token: Snip20,
    },
    ClaimRewardPool {
        recipient: Option<HumanAddr>,
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
    LockTokens { status: ResponseStatus },
    AddToRewardPool { status: ResponseStatus },
    Redeem { status: ResponseStatus },
    WithdrawRewards { status: ResponseStatus },
    CreateViewingKey { key: ViewingKey },
    SetViewingKey { status: ResponseStatus },
    UpdateIncentivizedToken { status: ResponseStatus },
    UpdateRewardToken { status: ResponseStatus },
    StopContract { status: ResponseStatus },
    ResumeContract { status: ResponseStatus },
    ChangeAdmin { status: ResponseStatus },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    QueryUnlockClaimHeight {},
    QueryContractStatus {},
    QueryRewardToken {},
    QueryIncentivizedToken {},

    // Authenticated
    QueryRewards { address: HumanAddr, key: String },
    QueryDeposit { address: HumanAddr, key: String },
}

impl QueryMsg {
    pub fn get_validation_params(&self) -> (&HumanAddr, ViewingKey) {
        match self {
            QueryMsg::QueryRewards { address, key } => (address, ViewingKey(key.clone())),
            QueryMsg::QueryDeposit { address, key } => (address, ViewingKey(key.clone())),
            _ => panic!("This should never happen"),
        }
    }
}

#[derive(Serialize, Deserialize, JsonSchema, Debug)]
#[serde(rename_all = "snake_case")]
pub enum QueryAnswer {
    QueryRewards { rewards: Uint128 },
    QueryDeposit { deposit: Uint128 },
    QueryUnlockClaimHeight { height: Uint128 },
    QueryContractStatus { is_stopped: bool },
    QueryRewardToken { token: Snip20 },
    QueryIncentivizedToken { token: Snip20 },

    QueryError { msg: String },
}

#[derive(Serialize, Deserialize, Clone, PartialEq, JsonSchema, Debug)]
#[serde(rename_all = "snake_case")]
pub enum ResponseStatus {
    Success,
    Failure,
}

// Messages sent to SNIP-20 contracts
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Snip20Msg {
    RegisterReceive {
        code_hash: String,
        padding: Option<String>,
    },
    Transfer {
        recipient: HumanAddr,
        amount: Uint128,
        padding: Option<String>,
    },
}

impl Snip20Msg {
    pub fn register_receive(code_hash: String) -> Self {
        Snip20Msg::RegisterReceive {
            code_hash,
            padding: None,
        }
    }

    pub fn transfer(recipient: HumanAddr, amount: Uint128) -> Self {
        Snip20Msg::Transfer {
            recipient,
            amount,
            padding: None,
        }
    }
}
