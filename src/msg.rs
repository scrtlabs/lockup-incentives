use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InitMsg {}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HandleMsg {
    Register {},
    Redeem {},
    WithdrawRewards {},

    // Admin commands
    Stop {},
    Resume {},
    AddTokenToTrack {},
    AdjustTokenWeights {},
    DistributeRewards {},
    ChangeAdmin {},
    // TODO: WithdrawBalance? (secretSCRT)
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    Rewards {},
    TotalDistributedRewards {},
}
