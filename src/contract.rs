use cosmwasm_std::{
    from_binary, to_binary, Api, Binary, CosmosMsg, Env, Extern, HandleResponse, HumanAddr,
    InitResponse, Querier, StdError, StdResult, Storage, Uint128, WasmMsg,
};
use secret_toolkit::storage::{TypedStore, TypedStoreMut};

use crate::constants::*;
use crate::msg::ResponseStatus::Success;
use crate::msg::{HandleAnswer, HandleMsg, InitMsg, QueryMsg, Snip20Msg};
use crate::state::{Config, Lockup, Lockups, Snip20};

pub fn init<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    msg: InitMsg,
) -> StdResult<InitResponse> {
    // Initialize state
    {
        let mut lockups_store = TypedStoreMut::attach(&mut deps.storage);
        lockups_store.store(LOCKUPS_KEY, &Lockups::new())?;
    }
    {
        let mut config_store = TypedStoreMut::attach(&mut deps.storage);
        config_store.store(
            CONFIG_KEY,
            &Config {
                admin: env.message.sender.clone(),
                reward_token: msg.reward_token.clone(),
                incentivized: msg.incentivized.clone(),
            },
        )?;
    }
    {
        let mut pool_store = TypedStoreMut::attach(&mut deps.storage);
        pool_store.store(REWARD_POOL_KEY, &0u128)?;
    }

    // Register sSCRT and incentivized token
    let register_msgs = vec![
        register(env.clone(), msg.reward_token)?,
        register(env.clone(), msg.incentivized)?,
    ];

    Ok(InitResponse {
        messages: register_msgs,
        log: vec![],
    })
}

pub fn handle<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    msg: HandleMsg,
) -> StdResult<HandleResponse> {
    match msg {
        HandleMsg::Redeem { .. } => unimplemented!(),
        HandleMsg::Receive { .. } => unimplemented!(),
        _ => unimplemented!(),
    }
}

pub fn query<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    msg: QueryMsg,
) -> StdResult<Binary> {
    match msg {};
    unimplemented!()
}

fn lock_tokens<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    from: HumanAddr,
    amount: u128,
) -> StdResult<HandleResponse> {
    {
        let config: Config = TypedStoreMut::attach(&mut deps.storage).load(CONFIG_KEY)?;
        if env.message.sender != config.incentivized.address {
            return Err(StdError::generic_err(format!(
                "This token is not supported. Supported: {}, given: {}",
                env.message.sender, config.incentivized.address
            )));
        }
    }

    let mut store = TypedStoreMut::attach(&mut deps.storage);
    let mut lockups: Lockups = store.load(LOCKUPS_KEY)?;

    if let Some(user_lockup) = lockups.get_mut(&from) {
        if let Some(new_amount) = user_lockup.locked.checked_add(amount) {
            user_lockup.locked = new_amount;
        }
    } else {
        lockups.insert(
            from,
            Lockup {
                locked: amount,
                pending_rewards: 0,
            },
        );
    }

    store.store(LOCKUPS_KEY, &lockups)?;

    Ok(HandleResponse {
        messages: vec![],
        log: vec![],
        data: Some(to_binary(&HandleAnswer::LockTokens { status: Success })?),
    })
}

fn add_to_pool<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    amount: u128,
) -> StdResult<HandleResponse> {
    {
        let config: Config = TypedStoreMut::attach(&mut deps.storage).load(CONFIG_KEY)?;
        if env.message.sender != config.reward_token.address {
            return Err(StdError::generic_err(format!(
                "This token is not supported. Supported: {}, given: {}",
                env.message.sender, config.incentivized.address
            )));
        }
    }

    let mut pool_store = TypedStoreMut::attach(&mut deps.storage);
    let pool: u128 = pool_store.load(REWARD_POOL_KEY)?;

    if let Some(new_pool_balance) = pool.checked_add(amount) {
        pool_store.store(REWARD_POOL_KEY, &new_pool_balance)?;
    }

    Ok(HandleResponse {
        messages: vec![],
        log: vec![],
        data: Some(to_binary(&HandleAnswer::LockTokens { status: Success })?),
    })
}

fn register(env: Env, token: Snip20) -> StdResult<CosmosMsg> {
    let msg = to_binary(&Snip20Msg::register_receive(env.contract_code_hash))?;
    let message = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: token.address,
        callback_code_hash: token.contract_hash,
        msg,
        send: vec![],
    });

    Ok(message)
}

fn receive<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    from: HumanAddr,
    amount: u128,
    msg: Binary,
) -> StdResult<HandleResponse> {
    let msg: HandleMsg = from_binary(&msg)?;

    if matches!(msg, HandleMsg::Receive { .. }) {
        return Err(StdError::generic_err(
            "Recursive call to receive() is not allowed",
        ));
    }

    match msg {
        HandleMsg::LockTokens {} => lock_tokens(deps, env, from, amount),
        HandleMsg::AddToRewardPool {} => add_to_pool(deps, env, amount),
        _ => Err(StdError::generic_err("Illegal internal receive message")),
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
