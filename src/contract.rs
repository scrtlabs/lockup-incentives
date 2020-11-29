use cosmwasm_std::{
    from_binary, to_binary, Api, BankMsg, Binary, BlockInfo, Coin, CosmosMsg, Env, Extern,
    HandleResponse, HumanAddr, InitResponse, Querier, ReadonlyStorage, StdError, StdResult,
    Storage, Uint128, WasmMsg, WasmQuery,
};
use secret_toolkit::snip20;
use secret_toolkit::storage::{TypedStore, TypedStoreMut};

use crate::constants::*;
use crate::msg::ResponseStatus::Success;
use crate::msg::{HandleAnswer, HandleMsg, InitMsg, QueryAnswer, QueryMsg, Snip20Msg};
use crate::state::{Config, Lockup, Lockups, RewardPool, Snip20, UserInfo};
use crate::viewing_key::{ViewingKey, VIEWING_KEY_SIZE};
use cosmwasm_storage::{PrefixedStorage, ReadonlyPrefixedStorage, TypedStorage};
use secret_toolkit::crypto::sha_256;

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
        let prng_seed_hashed = sha_256(&msg.prng_seed.0);
        let mut config_store = TypedStoreMut::attach(&mut deps.storage);
        config_store.store(
            CONFIG_KEY,
            &Config {
                admin: env.message.sender.clone(),
                reward_token: msg.reward_token.clone(),
                incentivized: msg.incentivized.clone(),
                pool_claim_height: msg.pool_claim_block,
                end_by_height: msg.end_by_height,
                viewing_key: msg.viewing_key.clone(),
                prng_seed: prng_seed_hashed.to_vec(),
                is_stopped: false,
            },
        )?;
    }
    {
        let mut pool_store = TypedStoreMut::attach(&mut deps.storage);
        pool_store.store(REWARD_POOL_KEY, &0u128)?;
    }
    // {
    //     let mut vk_store = TypedStoreMut::attach(&mut deps.storage);
    //     vk_store.store(REWARD_POOL_KEY, &0u128)?;
    // }

    // Register sSCRT and incentivized token, set vks
    let messages = vec![
        register(env.clone(), msg.reward_token.clone())?,
        register(env.clone(), msg.incentivized.clone())?,
        snip20::handle::set_viewing_key_msg(
            msg.viewing_key.clone(),
            None,
            1,
            env.contract_code_hash.clone(),
            msg.reward_token.address,
        )?,
        snip20::handle::set_viewing_key_msg(
            msg.viewing_key,
            None,
            1,
            env.contract_code_hash,
            msg.incentivized.address,
        )?,
    ];

    Ok(InitResponse {
        messages,
        log: vec![],
    })
}

pub fn handle<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    msg: HandleMsg,
) -> StdResult<HandleResponse> {
    let config: Config = TypedStoreMut::attach(&mut deps.storage).load(CONFIG_KEY)?;
    if config.is_stopped {
        return match msg {
            HandleMsg::Redeem { amount } => redeem(deps, env, amount),
            HandleMsg::WithdrawRewards {} => withdraw_rewards(deps, env),
            HandleMsg::ResumeContract {} => resume_contract(deps, env),
            // TODO: Add more messages here
            _ => Err(StdError::generic_err(
                "This contract is stopped and this action is not allowed",
            )),
        };
    }

    match msg {
        HandleMsg::Redeem { amount } => redeem(deps, env, amount),
        HandleMsg::Receive {
            from, amount, msg, ..
        } => receive(deps, env, from, amount.u128(), msg),
        HandleMsg::WithdrawRewards {} => withdraw_rewards(deps, env),
        HandleMsg::CreateViewingKey { entropy, .. } => create_viewing_key(deps, env, entropy),
        HandleMsg::SetViewingKey { key, .. } => set_viewing_key(deps, env, key),
        HandleMsg::UpdateIncentivizedToken { new_token } => update_inc_token(deps, env, new_token),
        HandleMsg::UpdateRewardToken { new_token } => update_reward_token(deps, env, new_token),
        HandleMsg::ClaimRewardPool { recipient } => claim_reward_pool(deps, env, recipient),
        HandleMsg::StopContract {} => stop_contract(deps, env),
        HandleMsg::ChangeAdmin { address } => change_admin(deps, env, address),
        _ => Err(StdError::generic_err("Unavailable or unknown action")),
    }
}

pub fn query<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    msg: QueryMsg,
) -> StdResult<Binary> {
    match msg {
        QueryMsg::QueryUnlockClaimHeight {} => query_claim_unlock_height(deps),
        QueryMsg::QueryContractStatus {} => query_contract_status(deps),
        QueryMsg::QueryRewardToken {} => query_reward_token(deps),
        QueryMsg::QueryIncentivizedToken {} => query_incentivized_token(deps),
        _ => authenticated_queries(deps, msg),
    }
}

pub fn authenticated_queries<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    msg: QueryMsg,
) -> StdResult<Binary> {
    let (address, key) = msg.get_validation_params();

    let vk_store = ReadonlyPrefixedStorage::new(VIEWING_KEY_KEY, &deps.storage);
    let expected_key = vk_store.get(address.0.as_bytes());

    if expected_key.is_none() {
        // Checking the key will take significant time. We don't want to exit immediately if it isn't set
        // in a way which will allow to time the command and determine if a viewing key doesn't exist
        key.check_viewing_key(&[0u8; VIEWING_KEY_SIZE]);
    } else if key.check_viewing_key(expected_key.unwrap().as_slice()) {
        return match msg {
            QueryMsg::QueryRewards { address, .. } => query_rewards(deps, &address),
            QueryMsg::QueryDeposit { address, .. } => query_deposit(deps, &address),
            _ => panic!("This should never happen"),
        };
    }

    Ok(to_binary(&QueryAnswer::QueryError {
        msg: "Wrong viewing key for this address or viewing key not set".to_string(),
    })?)
}

// Handle functions

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

fn lock_tokens<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    from: HumanAddr,
    amount: u128,
) -> StdResult<HandleResponse> {
    // Ensure that the sent tokens are from an expected contract address
    let config = TypedStore::<Config, S>::attach(&deps.storage).load(CONFIG_KEY)?;
    if env.message.sender != config.incentivized.address {
        return Err(StdError::generic_err(format!(
            "This token is not supported. Supported: {}, given: {}",
            env.message.sender, config.incentivized.address
        )));
    }

    let reward_pool = update_rewards(deps, &env, &config)?;

    let mut messages: Vec<CosmosMsg> = vec![];
    let mut users_store = TypedStoreMut::<UserInfo, S>::attach(&mut deps.storage);
    let mut user = users_store
        .load(from.0.as_bytes())
        .unwrap_or(UserInfo { locked: 0, debt: 0 }); // NotFound is the only possible error

    if user.locked > 0 {
        let pending = user.locked * reward_pool.acc_reward_per_share - user.debt;
        if pending > 0 {
            messages.push(secret_toolkit::snip20::transfer_msg(
                from.clone(),
                Uint128(pending),
                None,
                RESPONSE_BLOCK_SIZE,
                config.reward_token.contract_hash,
                config.reward_token.address,
            )?);
        }
    }

    user.locked += amount;
    user.debt = user.locked * reward_pool.acc_reward_per_share;
    users_store.store(from.0.as_bytes(), &user)?;

    Ok(HandleResponse {
        messages,
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
        data: Some(to_binary(&HandleAnswer::AddToRewardPool {
            status: Success,
        })?),
    })
}

fn redeem<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    amount: Option<Uint128>,
) -> StdResult<HandleResponse> {
    let config = TypedStore::<Config, S>::attach(&deps.storage).load(CONFIG_KEY)?;
    let mut user = TypedStore::<UserInfo, S>::attach(&deps.storage)
        .load(env.message.sender.0.as_bytes())
        .unwrap_or(UserInfo { locked: 0, debt: 0 }); // NotFound is the only possible error
    let amount = amount.unwrap_or(Uint128(user.locked)).u128();

    if amount > user.locked {
        return Err(StdError::generic_err(format!(
            "insufficient funds to redeem: balance={}, required={}",
            user.locked, amount,
        )));
    }

    let mut messages: Vec<CosmosMsg> = vec![];
    let reward_pool = update_rewards(deps, &env, &config)?;
    let pending = user.locked * reward_pool.acc_reward_per_share - user.debt;
    if pending > 0 {
        // Transfer rewards
        messages.push(secret_toolkit::snip20::transfer_msg(
            env.message.sender.clone(),
            Uint128(pending),
            None,
            RESPONSE_BLOCK_SIZE,
            config.reward_token.contract_hash,
            config.reward_token.address,
        )?);
    }

    // Transfer redeemed tokens
    user.locked -= amount;
    messages.push(secret_toolkit::snip20::transfer_msg(
        env.message.sender.clone(),
        Uint128(amount),
        None,
        RESPONSE_BLOCK_SIZE,
        config.incentivized.contract_hash,
        config.incentivized.address,
    )?);

    user.debt = user.locked * reward_pool.acc_reward_per_share;
    TypedStoreMut::<UserInfo, S>::attach(&mut deps.storage)
        .store(env.message.sender.clone().0.as_bytes(), &user)?;

    let mut lockup_store = TypedStoreMut::attach(&mut deps.storage);
    let mut lockups: Lockups = lockup_store.load(LOCKUPS_KEY)?;

    Ok(HandleResponse {
        messages,
        log: vec![],
        data: None,
    })
}

fn withdraw_rewards<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
) -> StdResult<HandleResponse> {
    let config: Config;
    {
        config = TypedStoreMut::attach(&mut deps.storage).load(CONFIG_KEY)?;
    }

    let mut lockup_store = TypedStoreMut::attach(&mut deps.storage);
    let mut lockups: Lockups = lockup_store.load(LOCKUPS_KEY)?;

    let rewards: u128;
    if let Some(user_lockup) = lockups.get_mut(&env.message.sender) {
        rewards = user_lockup.pending_rewards;
        user_lockup.pending_rewards = 0;
    } else {
        return Err(StdError::generic_err(format!(
            "no assets locked for: {}",
            env.message.sender
        )));
    }

    lockup_store.store(LOCKUPS_KEY, &lockups)?;

    Ok(HandleResponse {
        messages: vec![transfer(env.message.sender, config.incentivized, rewards)?],
        log: vec![],
        data: None,
    })
}

pub fn create_viewing_key<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    entropy: String,
) -> StdResult<HandleResponse> {
    let config: Config = TypedStoreMut::attach(&mut deps.storage).load(CONFIG_KEY)?;
    let prng_seed = config.prng_seed;

    let key = ViewingKey::new(&env, &prng_seed, (&entropy).as_ref());

    let mut vk_store = PrefixedStorage::new(VIEWING_KEY_KEY, &mut deps.storage);
    vk_store.set(env.message.sender.0.as_bytes(), &key.to_hashed());

    Ok(HandleResponse {
        messages: vec![],
        log: vec![],
        data: Some(to_binary(&HandleAnswer::CreateViewingKey { key })?),
    })
}

pub fn set_viewing_key<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    key: String,
) -> StdResult<HandleResponse> {
    let vk = ViewingKey(key);

    let mut vk_store = PrefixedStorage::new(VIEWING_KEY_KEY, &mut deps.storage);
    vk_store.set(env.message.sender.0.as_bytes(), &vk.to_hashed());

    Ok(HandleResponse {
        messages: vec![],
        log: vec![],
        data: Some(to_binary(&HandleAnswer::SetViewingKey { status: Success })?),
    })
}

fn update_inc_token<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    new_token: Snip20,
) -> StdResult<HandleResponse> {
    let mut config_store = TypedStoreMut::attach(&mut deps.storage);
    let mut config: Config = config_store.load(CONFIG_KEY)?;

    enforce_admin(config.clone(), env)?;

    config.incentivized = new_token.clone();
    config_store.store(CONFIG_KEY, &config)?;

    Ok(HandleResponse {
        messages: vec![],
        log: vec![],
        data: Some(to_binary(&HandleAnswer::UpdateIncentivizedToken {
            status: Success,
        })?),
    })
}

fn update_reward_token<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    new_token: Snip20,
) -> StdResult<HandleResponse> {
    let mut config_store = TypedStoreMut::attach(&mut deps.storage);
    let mut config: Config = config_store.load(CONFIG_KEY)?;

    enforce_admin(config.clone(), env)?;

    config.reward_token = new_token.clone();
    config_store.store(CONFIG_KEY, &config)?;

    Ok(HandleResponse {
        messages: vec![],
        log: vec![],
        data: Some(to_binary(&HandleAnswer::UpdateRewardToken {
            status: Success,
        })?),
    })
}

fn claim_reward_pool<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    recipient: Option<HumanAddr>,
) -> StdResult<HandleResponse> {
    let mut config_store = TypedStoreMut::attach(&mut deps.storage);
    let mut config: Config = config_store.load(CONFIG_KEY)?;

    enforce_admin(config.clone(), env.clone())?;

    if env.block.height < config.pool_claim_height {
        return Err(StdError::generic_err(format!(
            "minimum claim height hasn't passed yet: {}",
            config.pool_claim_height
        )));
    }

    let total_rewards = snip20::balance_query(
        &deps.querier,
        env.contract.address,
        config.viewing_key,
        RESPONSE_BLOCK_SIZE,
        env.contract_code_hash,
        config.reward_token.address.clone(),
    )?;

    Ok(HandleResponse {
        messages: vec![transfer(
            recipient.unwrap_or(env.message.sender),
            config.reward_token,
            total_rewards.amount.u128(),
        )?],
        log: vec![],
        data: None,
    })
}

fn stop_contract<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
) -> StdResult<HandleResponse> {
    let mut config_store = TypedStoreMut::attach(&mut deps.storage);
    let mut config: Config = config_store.load(CONFIG_KEY)?;

    enforce_admin(config.clone(), env)?;

    config.is_stopped = true;
    config_store.store(CONFIG_KEY, &config)?;

    Ok(HandleResponse {
        messages: vec![],
        log: vec![],
        data: Some(to_binary(&HandleAnswer::StopContract { status: Success })?),
    })
}

fn resume_contract<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
) -> StdResult<HandleResponse> {
    let mut config_store = TypedStoreMut::attach(&mut deps.storage);
    let mut config: Config = config_store.load(CONFIG_KEY)?;

    enforce_admin(config.clone(), env)?;

    config.is_stopped = false;
    config_store.store(CONFIG_KEY, &config)?;

    Ok(HandleResponse {
        messages: vec![],
        log: vec![],
        data: Some(to_binary(&HandleAnswer::ResumeContract {
            status: Success,
        })?),
    })
}

fn change_admin<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    address: HumanAddr,
) -> StdResult<HandleResponse> {
    let mut config_store = TypedStoreMut::attach(&mut deps.storage);
    let mut config: Config = config_store.load(CONFIG_KEY)?;

    enforce_admin(config.clone(), env)?;

    config.admin = address;
    config_store.store(CONFIG_KEY, &config)?;

    Ok(HandleResponse {
        messages: vec![],
        log: vec![],
        data: Some(to_binary(&HandleAnswer::ChangeAdmin { status: Success })?),
    })
}

// Query functions

fn query_rewards<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    address: &HumanAddr,
) -> StdResult<Binary> {
    let lockups: Lockups = TypedStore::attach(&deps.storage).load(LOCKUPS_KEY)?;

    let amount = match lockups.get(address) {
        None => 0,
        Some(lockup) => lockup.pending_rewards,
    };

    to_binary(&QueryAnswer::QueryRewards {
        rewards: Uint128(amount),
    })
}

fn query_deposit<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    address: &HumanAddr,
) -> StdResult<Binary> {
    let lockups: Lockups = TypedStore::attach(&deps.storage).load(LOCKUPS_KEY)?;

    let amount = match lockups.get(address) {
        None => 0,
        Some(lockup) => lockup.locked,
    };

    to_binary(&QueryAnswer::QueryDeposit {
        deposit: Uint128(amount),
    })
}

fn query_claim_unlock_height<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
) -> StdResult<Binary> {
    let config: Config = TypedStore::attach(&deps.storage).load(CONFIG_KEY)?;

    to_binary(&QueryAnswer::QueryUnlockClaimHeight {
        height: Uint128(config.pool_claim_height as u128),
    })
}

fn query_contract_status<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
) -> StdResult<Binary> {
    let config: Config = TypedStore::attach(&deps.storage).load(CONFIG_KEY)?;

    to_binary(&QueryAnswer::QueryContractStatus {
        is_stopped: config.is_stopped,
    })
}

fn query_reward_token<S: Storage, A: Api, Q: Querier>(deps: &Extern<S, A, Q>) -> StdResult<Binary> {
    let config: Config = TypedStore::attach(&deps.storage).load(CONFIG_KEY)?;

    to_binary(&QueryAnswer::QueryRewardToken {
        token: config.reward_token,
    })
}

fn query_incentivized_token<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
) -> StdResult<Binary> {
    let config: Config = TypedStore::attach(&deps.storage).load(CONFIG_KEY)?;

    to_binary(&QueryAnswer::QueryIncentivizedToken {
        token: config.incentivized,
    })
}

// Helper functions

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

fn transfer(recipient: HumanAddr, token: Snip20, amount: u128) -> StdResult<CosmosMsg> {
    let msg = to_binary(&Snip20Msg::transfer(recipient, Uint128(amount)))?;
    let message = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: token.address,
        callback_code_hash: token.contract_hash,
        msg,
        send: vec![],
    });

    Ok(message)
}

fn enforce_admin(config: Config, env: Env) -> StdResult<()> {
    if config.admin != env.message.sender {
        return Err(StdError::generic_err(format!(
            "no assets locked for: {}",
            env.message.sender
        )));
    }

    Ok(())
}

fn update_rewards<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: &Env,
    config: &Config,
) -> StdResult<RewardPool> {
    let mut rewards_store = TypedStoreMut::attach(&mut deps.storage);
    let mut reward_pool: RewardPool = rewards_store.load(REWARD_POOL_KEY)?;

    if env.block.height <= reward_pool.last_reward_block || env.block.height > config.end_by_height
    {
        return Ok(reward_pool.clone());
    }

    if reward_pool.inc_token_supply == 0 || reward_pool.pending_rewards == 0 {
        reward_pool.last_reward_block = env.block.height;
        rewards_store.store(REWARD_POOL_KEY, &reward_pool)?;
        return Ok(reward_pool.clone());
    }

    let blocks_to_go = config.end_by_height - reward_pool.last_reward_block;
    let blocks_to_vest = env.block.height - reward_pool.last_reward_block;
    let rewards = blocks_to_vest as u128 * (reward_pool.pending_rewards / (blocks_to_go as u128));

    reward_pool.acc_reward_per_share += rewards / reward_pool.inc_token_supply;

    reward_pool.last_reward_block = env.block.height;
    rewards_store.store(REWARD_POOL_KEY, &reward_pool)?;

    Ok(reward_pool.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::testing::{mock_dependencies, mock_env};
    use cosmwasm_std::{coins, from_binary, StdError};

    #[test]
    fn proper_initialization() {}
}
