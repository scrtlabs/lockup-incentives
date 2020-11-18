use cosmwasm_std::{
    to_binary, Api, Binary, Env, Extern, HandleResponse, HumanAddr, InitResponse, Querier,
    StdError, StdResult, Storage,
};

use crate::msg::{CountResponse, HandleMsg, InitMsg, QueryMsg};
use crate::state::{config, config_read, Config, Lockup, Lockups, State};
use cosmwasm_storage::TypedStorage;
use secret_toolkit::storage::TypedStoreMut;
use std::collections::HashMap;

const LOCKUPS_KEY: &[u8] = b"lockups";
const CONFIG_KEY: &[u8] = b"config";

pub fn init<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    msg: InitMsg,
) -> StdResult<InitResponse> {
    let mut store_lockups = TypedStoreMut::attach(&mut deps.storage);
    let mut store_config = TypedStoreMut::attach(&mut deps.storage);
    store_lockups.store(LOCKUPS_KEY, &Lockups::new());
    store_config.store(
        CONFIG_KEY,
        &Config {
            admin: env.message.sender,
        },
    );

    Ok(InitResponse::default())
}

pub fn handle<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    msg: HandleMsg,
) -> StdResult<HandleResponse> {
    match msg {
        HandleMsg::Register { .. } => unimplemented!(),
        HandleMsg::Redeem { .. } => unimplemented!(),
        HandleMsg::WithdrawRewards { .. } => unimplemented!(),
        HandleMsg::Stop { .. } => unimplemented!(),
        HandleMsg::Resume { .. } => unimplemented!(),
        HandleMsg::AddTokenToTrack { .. } => unimplemented!(),
        HandleMsg::AdjustTokenWeights { .. } => unimplemented!(),
        HandleMsg::DistributeRewards { .. } => unimplemented!(),
        HandleMsg::ChangeAdmin { .. } => unimplemented!(),
    }
}

pub fn query<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    msg: QueryMsg,
) -> StdResult<Binary> {
    match msg {
        QueryMsg::Rewards { .. } => unimplemented!(),
        QueryMsg::TotalDistributedRewards { .. } => unimplemented!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::testing::{mock_dependencies, mock_env};
    use cosmwasm_std::{coins, from_binary, StdError};

    #[test]
    fn proper_initialization() {}
}
