use cosmwasm_std::{
    from_binary, to_binary, Api, Binary, CosmosMsg, Env, Extern, HandleResponse, HumanAddr,
    InitResponse, Querier, ReadonlyStorage, StdError, StdResult, Storage, Uint128,
};
use cosmwasm_storage::{PrefixedStorage, ReadonlyPrefixedStorage};
use secret_toolkit::crypto::sha_256;
use secret_toolkit::snip20;
use secret_toolkit::storage::{TypedStore, TypedStoreMut};
use secret_toolkit::utils::{pad_handle_result, pad_query_result};

use crate::constants::*;
use crate::msg::ResponseStatus::Success;
use crate::msg::{
    HandleAnswer, HandleMsg, InitMsg, QueryAnswer, QueryMsg, ReceiveAnswer, ReceiveMsg,
};
use crate::state::{Config, RewardPool, UserInfo};
use crate::viewing_key::{ViewingKey, VIEWING_KEY_SIZE};

pub fn init<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    msg: InitMsg,
) -> StdResult<InitResponse> {
    // Initialize state
    let prng_seed_hashed = sha_256(&msg.prng_seed.0);
    let mut config_store = TypedStoreMut::attach(&mut deps.storage);
    config_store.store(
        CONFIG_KEY,
        &Config {
            admin: env.message.sender.clone(),
            reward_token: msg.reward_token.clone(),
            inc_token: msg.inc_token.clone(),
            pool_claim_block: msg.pool_claim_block,
            deadline: msg.deadline,
            viewing_key: msg.viewing_key.clone(),
            prng_seed: prng_seed_hashed.to_vec(),
            is_stopped: false,
        },
    )?;

    TypedStoreMut::<RewardPool, S>::attach(&mut deps.storage).store(
        REWARD_POOL_KEY,
        &RewardPool {
            pending_rewards: 0,
            inc_token_supply: 0,
            last_reward_block: 0,
            acc_reward_per_share: 0,
        },
    )?;

    // Register sSCRT and incentivized token, set vks
    let messages = vec![
        snip20::register_receive_msg(
            env.contract_code_hash.clone(),
            None,
            1, // This is public data, no need to pad
            msg.reward_token.contract_hash.clone(),
            msg.reward_token.address.clone(),
        )?,
        snip20::register_receive_msg(
            env.contract_code_hash,
            None,
            1,
            msg.inc_token.contract_hash.clone(),
            msg.inc_token.address.clone(),
        )?,
        snip20::set_viewing_key_msg(
            msg.viewing_key.clone(),
            None,
            RESPONSE_BLOCK_SIZE, // This is private data, need to pad
            msg.reward_token.contract_hash,
            msg.reward_token.address,
        )?,
        snip20::set_viewing_key_msg(
            msg.viewing_key,
            None,
            RESPONSE_BLOCK_SIZE,
            msg.inc_token.contract_hash,
            msg.inc_token.address,
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
            HandleMsg::EmergencyRedeem {} => emergency_redeem(deps, env),
            HandleMsg::ResumeContract {} => resume_contract(deps, env),
            _ => Err(StdError::generic_err(
                "this contract is stopped and this action is not allowed",
            )),
        };
    }

    let response = match msg {
        HandleMsg::Redeem { amount } => redeem(deps, env, amount),
        HandleMsg::Receive {
            from, amount, msg, ..
        } => receive(deps, env, from, amount.u128(), msg),
        HandleMsg::CreateViewingKey { entropy, .. } => create_viewing_key(deps, env, entropy),
        HandleMsg::SetViewingKey { key, .. } => set_viewing_key(deps, env, key),
        HandleMsg::ClaimRewardPool { to: recipient } => claim_reward_pool(deps, env, recipient),
        HandleMsg::StopContract {} => stop_contract(deps, env),
        HandleMsg::ChangeAdmin { address } => change_admin(deps, env, address),
        HandleMsg::SetDeadline { block: height } => set_deadline(deps, env, height),
        _ => Err(StdError::generic_err("Unavailable or unknown action")),
    };

    pad_handle_result(response, RESPONSE_BLOCK_SIZE)
}

pub fn query<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    msg: QueryMsg,
) -> StdResult<Binary> {
    let response = match msg {
        QueryMsg::ClaimBlock {} => query_claim_block(deps),
        QueryMsg::ContractStatus {} => query_contract_status(deps),
        QueryMsg::RewardToken {} => query_reward_token(deps),
        QueryMsg::IncentivizedToken {} => query_incentivized_token(deps),
        QueryMsg::EndHeight {} => query_end_height(deps),
        QueryMsg::RewardPoolBalance {} => query_reward_pool_balance(deps),
        QueryMsg::TokenInfo {} => query_token_info(),
        _ => authenticated_queries(deps, msg),
    };

    pad_query_result(response, RESPONSE_BLOCK_SIZE)
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
            QueryMsg::Rewards {
                address, height, ..
            } => query_pending_rewards(deps, &address, height),
            QueryMsg::Deposit { address, .. } => query_deposit(deps, &address),
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
    let msg: ReceiveMsg = from_binary(&msg)?;

    match msg {
        ReceiveMsg::Deposit {} => deposit(deps, env, from, amount),
        ReceiveMsg::DepositRewards {} => deposit_rewards(deps, env, amount),
    }
}

fn deposit<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    from: HumanAddr,
    amount: u128,
) -> StdResult<HandleResponse> {
    // Ensure that the sent tokens are from an expected contract address
    let config = TypedStore::<Config, S>::attach(&deps.storage).load(CONFIG_KEY)?;
    if env.message.sender != config.inc_token.address {
        return Err(StdError::generic_err(format!(
            "This token is not supported. Supported: {}, given: {}",
            config.inc_token.address, env.message.sender
        )));
    }

    // Adjust scale to allow easy division and prevent overflows
    let amount = amount / INC_TOKEN_SCALE;

    let mut reward_pool = update_rewards(deps, &env, &config)?;

    let mut messages: Vec<CosmosMsg> = vec![];
    let mut users_store = TypedStoreMut::<UserInfo, S>::attach(&mut deps.storage);
    let mut user = users_store
        .load(from.0.as_bytes())
        .unwrap_or(UserInfo { locked: 0, debt: 0 }); // NotFound is the only possible error

    if user.locked > 0 {
        let pending = user.locked * reward_pool.acc_reward_per_share / REWARD_SCALE - user.debt;
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
    user.debt = user.locked * reward_pool.acc_reward_per_share / REWARD_SCALE;
    users_store.store(from.0.as_bytes(), &user)?;

    reward_pool.inc_token_supply += amount;
    TypedStoreMut::attach(&mut deps.storage).store(REWARD_POOL_KEY, &reward_pool)?;

    Ok(HandleResponse {
        messages,
        log: vec![],
        data: Some(to_binary(&ReceiveAnswer::Deposit { status: Success })?),
    })
}

fn deposit_rewards<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    amount: u128,
) -> StdResult<HandleResponse> {
    let config = TypedStore::<Config, S>::attach(&deps.storage).load(CONFIG_KEY)?;
    if env.message.sender != config.reward_token.address {
        return Err(StdError::generic_err(format!(
            "This token is not supported. Supported: {}, given: {}",
            config.reward_token.address, env.message.sender
        )));
    }

    let mut reward_pool = update_rewards(deps, &env, &config)?;

    reward_pool.pending_rewards += amount - 1_000_000; // Subtracting 1scrt just to give room for rounding errors in calculations
    TypedStoreMut::attach(&mut deps.storage).store(REWARD_POOL_KEY, &reward_pool)?;

    Ok(HandleResponse {
        messages: vec![],
        log: vec![],
        data: Some(to_binary(&ReceiveAnswer::DepositRewards {
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
    let amount = amount
        .unwrap_or(Uint128(user.locked * INC_TOKEN_SCALE)) // Multiplying to match scale of input, dividing again later
        .u128()
        / INC_TOKEN_SCALE;

    if amount > user.locked {
        return Err(StdError::generic_err(format!(
            "insufficient funds to redeem: balance={}, required={}",
            user.locked * INC_TOKEN_SCALE,
            amount * INC_TOKEN_SCALE,
        )));
    }

    let mut messages: Vec<CosmosMsg> = vec![];
    let mut reward_pool = update_rewards(deps, &env, &config)?;
    let pending = user.locked * reward_pool.acc_reward_per_share / REWARD_SCALE - user.debt;
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
    user.debt = user.locked * reward_pool.acc_reward_per_share / REWARD_SCALE;
    TypedStoreMut::<UserInfo, S>::attach(&mut deps.storage)
        .store(env.message.sender.0.as_bytes(), &user)?;

    reward_pool.inc_token_supply -= amount;
    TypedStoreMut::attach(&mut deps.storage).store(REWARD_POOL_KEY, &reward_pool)?;

    messages.push(secret_toolkit::snip20::transfer_msg(
        env.message.sender,
        Uint128(amount * INC_TOKEN_SCALE),
        None,
        RESPONSE_BLOCK_SIZE,
        config.inc_token.contract_hash,
        config.inc_token.address,
    )?);

    Ok(HandleResponse {
        messages,
        log: vec![],
        data: Some(to_binary(&HandleAnswer::Redeem { status: Success })?),
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

fn claim_reward_pool<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    recipient: Option<HumanAddr>,
) -> StdResult<HandleResponse> {
    let config_store = TypedStore::attach(&deps.storage);
    let config: Config = config_store.load(CONFIG_KEY)?;

    enforce_admin(config.clone(), env.clone())?;

    if env.block.height < config.pool_claim_block {
        return Err(StdError::generic_err(format!(
            "minimum claim height hasn't passed yet: {}",
            config.pool_claim_block
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
        messages: vec![snip20::transfer_msg(
            recipient.unwrap_or(env.message.sender),
            total_rewards.amount,
            None,
            RESPONSE_BLOCK_SIZE,
            config.reward_token.contract_hash,
            config.reward_token.address,
        )?],
        log: vec![],
        data: Some(to_binary(&HandleAnswer::ClaimRewardPool {
            status: Success,
        })?),
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

/// YOU SHOULD NEVER USE THIS! This will erase any eligibility for rewards you earned so far
fn emergency_redeem<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
) -> StdResult<HandleResponse> {
    let config: Config = TypedStore::attach(&deps.storage).load(CONFIG_KEY)?;
    let mut user: UserInfo = TypedStoreMut::attach(&mut deps.storage)
        .load(env.message.sender.0.as_bytes())
        .unwrap_or(UserInfo { locked: 0, debt: 0 });

    let mut reward_pool: RewardPool =
        TypedStoreMut::attach(&mut deps.storage).load(REWARD_POOL_KEY)?;
    reward_pool.inc_token_supply -= user.locked;
    TypedStoreMut::attach(&mut deps.storage).store(REWARD_POOL_KEY, &reward_pool)?;

    let mut messages = vec![];
    if user.locked > 0 {
        messages.push(secret_toolkit::snip20::transfer_msg(
            env.message.sender.clone(),
            Uint128(user.locked * INC_TOKEN_SCALE),
            None,
            RESPONSE_BLOCK_SIZE,
            config.inc_token.contract_hash,
            config.inc_token.address,
        )?);
    }

    user = UserInfo { locked: 0, debt: 0 };
    TypedStoreMut::attach(&mut deps.storage).store(env.message.sender.0.as_bytes(), &user)?;

    Ok(HandleResponse {
        messages,
        log: vec![],
        data: Some(to_binary(&HandleAnswer::EmergencyRedeem {
            status: Success,
        })?),
    })
}

fn set_deadline<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    height: u64,
) -> StdResult<HandleResponse> {
    let mut config = TypedStoreMut::<Config, S>::attach(&mut deps.storage).load(CONFIG_KEY)?;

    enforce_admin(config.clone(), env.clone())?;
    update_rewards(deps, &env, &config)?;

    config.deadline = height;
    TypedStoreMut::<Config, S>::attach(&mut deps.storage).store(CONFIG_KEY, &config)?;

    Ok(HandleResponse {
        messages: vec![],
        log: vec![],
        data: Some(to_binary(&HandleAnswer::SetDeadline { status: Success })?),
    })
}

// Query functions

fn query_pending_rewards<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    address: &HumanAddr,
    height: u64,
) -> StdResult<Binary> {
    let reward_pool = TypedStore::<RewardPool, S>::attach(&deps.storage).load(REWARD_POOL_KEY)?;
    let user = TypedStore::<UserInfo, S>::attach(&deps.storage)
        .load(address.0.as_bytes())
        .unwrap_or(UserInfo { locked: 0, debt: 0 });
    let config = TypedStore::<Config, S>::attach(&deps.storage).load(CONFIG_KEY)?;
    let mut acc_reward_per_share = reward_pool.acc_reward_per_share;

    if height > reward_pool.last_reward_block
        && reward_pool.last_reward_block < config.deadline
        && reward_pool.inc_token_supply != 0
    {
        let mut height = height;
        if height > config.deadline {
            height = config.deadline;
        }

        let blocks_to_go = config.deadline - reward_pool.last_reward_block;
        let blocks_to_vest = height - reward_pool.last_reward_block;
        let rewards =
            (blocks_to_vest as u128) * reward_pool.pending_rewards / (blocks_to_go as u128);

        acc_reward_per_share += rewards * REWARD_SCALE / reward_pool.inc_token_supply;
    }

    to_binary(&QueryAnswer::Rewards {
        // This is not necessarily accurate, since we don't validate the block height. It is up to
        // the UI to display accurate numbers
        rewards: Uint128(user.locked * acc_reward_per_share / REWARD_SCALE - user.debt),
    })
}

fn query_deposit<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    address: &HumanAddr,
) -> StdResult<Binary> {
    let user = TypedStore::attach(&deps.storage)
        .load(address.0.as_bytes())
        .unwrap_or(UserInfo { locked: 0, debt: 0 });

    to_binary(&QueryAnswer::Deposit {
        deposit: Uint128(user.locked * INC_TOKEN_SCALE),
    })
}

fn query_claim_block<S: Storage, A: Api, Q: Querier>(deps: &Extern<S, A, Q>) -> StdResult<Binary> {
    let config: Config = TypedStore::attach(&deps.storage).load(CONFIG_KEY)?;

    to_binary(&QueryAnswer::ClaimBlock {
        height: config.pool_claim_block,
    })
}

fn query_contract_status<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
) -> StdResult<Binary> {
    let config: Config = TypedStore::attach(&deps.storage).load(CONFIG_KEY)?;

    to_binary(&QueryAnswer::ContractStatus {
        is_stopped: config.is_stopped,
    })
}

fn query_reward_token<S: Storage, A: Api, Q: Querier>(deps: &Extern<S, A, Q>) -> StdResult<Binary> {
    let config: Config = TypedStore::attach(&deps.storage).load(CONFIG_KEY)?;

    to_binary(&QueryAnswer::RewardToken {
        token: config.reward_token,
    })
}

fn query_incentivized_token<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
) -> StdResult<Binary> {
    let config: Config = TypedStore::attach(&deps.storage).load(CONFIG_KEY)?;

    to_binary(&QueryAnswer::IncentivizedToken {
        token: config.inc_token,
    })
}

fn query_end_height<S: Storage, A: Api, Q: Querier>(deps: &Extern<S, A, Q>) -> StdResult<Binary> {
    let config: Config = TypedStore::attach(&deps.storage).load(CONFIG_KEY)?;

    to_binary(&QueryAnswer::EndHeight {
        height: config.deadline,
    })
}

fn query_reward_pool_balance<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
) -> StdResult<Binary> {
    let reward_pool: RewardPool = TypedStore::attach(&deps.storage).load(REWARD_POOL_KEY)?;

    to_binary(&QueryAnswer::RewardPoolBalance {
        balance: Uint128(reward_pool.pending_rewards as u128),
    })
}

// This is only for Keplr support (Viewing Keys)
fn query_token_info() -> StdResult<Binary> {
    to_binary(&QueryAnswer::TokenInfo {
        name: "ETH Bridge Rewards".to_string(),
        symbol: "ETH-RWRDS".to_string(),
        decimals: 1,
        total_supply: None,
    })
}

// Helper functions

fn enforce_admin(config: Config, env: Env) -> StdResult<()> {
    if config.admin != env.message.sender {
        return Err(StdError::generic_err(format!(
            "not an admin: {}",
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

    let mut block = env.block.height;
    if block > config.deadline {
        block = config.deadline;
    }

    if block <= reward_pool.last_reward_block || reward_pool.last_reward_block >= config.deadline {
        return Ok(reward_pool);
    }

    if reward_pool.inc_token_supply == 0 || reward_pool.pending_rewards == 0 {
        reward_pool.last_reward_block = block;
        rewards_store.store(REWARD_POOL_KEY, &reward_pool)?;
        return Ok(reward_pool);
    }

    let blocks_to_go = config.deadline - reward_pool.last_reward_block;
    let blocks_to_vest = block - reward_pool.last_reward_block;
    let rewards = (blocks_to_vest as u128) * reward_pool.pending_rewards / (blocks_to_go as u128);

    reward_pool.acc_reward_per_share += rewards * REWARD_SCALE / reward_pool.inc_token_supply;
    reward_pool.pending_rewards -= rewards;
    reward_pool.last_reward_block = block;
    rewards_store.store(REWARD_POOL_KEY, &reward_pool)?;

    Ok(reward_pool)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::msg::HandleMsg::{Receive, Redeem, SetViewingKey};
    use crate::msg::QueryMsg::{Deposit, Rewards};
    use crate::msg::ReceiveMsg;
    use crate::state::Snip20;
    use cosmwasm_std::testing::{
        mock_dependencies, MockApi, MockQuerier, MockStorage, MOCK_CONTRACT_ADDR,
    };
    use cosmwasm_std::{
        coins, from_binary, BlockInfo, Coin, ContractInfo, Empty, MessageInfo, StdError, WasmMsg,
    };
    use rand::Rng;
    use serde::{Deserialize, Serialize};

    // Helper functions

    fn init_helper(
        deadline: u64,
    ) -> (
        StdResult<InitResponse>,
        Extern<MockStorage, MockApi, MockQuerier>,
    ) {
        let mut deps = mock_dependencies(20, &[]);
        let env = mock_env("admin", &[], 1);

        let init_msg = InitMsg {
            reward_token: Snip20 {
                address: HumanAddr("scrt".to_string()),
                contract_hash: "1".to_string(),
            },
            inc_token: Snip20 {
                address: HumanAddr("eth".to_string()),
                contract_hash: "2".to_string(),
            },
            deadline,
            pool_claim_block: deadline + 1,
            prng_seed: Binary::from("lolz fun yay".as_bytes()),
            viewing_key: "123".to_string(),
        };

        (init(&mut deps, env, init_msg), deps)
    }

    /// Just set sender and sent funds for the message. The rest uses defaults.
    /// The sender will be canonicalized internally to allow developers pasing in human readable senders.
    /// This is intended for use in test code only.
    pub fn mock_env<U: Into<HumanAddr>>(sender: U, sent: &[Coin], height: u64) -> Env {
        Env {
            block: BlockInfo {
                height,
                time: 1_571_797_419,
                chain_id: "cosmos-testnet-14002".to_string(),
            },
            message: MessageInfo {
                sender: sender.into(),
                sent_funds: sent.to_vec(),
            },
            contract: ContractInfo {
                address: HumanAddr::from(MOCK_CONTRACT_ADDR),
            },
            contract_key: Some("".to_string()),
            contract_code_hash: "".to_string(),
        }
    }

    fn msg_from_action<S: Storage, A: Api, Q: Querier>(
        deps: &Extern<S, A, Q>,
        action: &str,
        user: HumanAddr,
    ) -> (HandleMsg, String) {
        let mut rng = rand::thread_rng();
        let chance = rng.gen_range(0, 100000);

        match action {
            "deposit" => {
                let amount: u128 = rng.gen_range(10e12 as u128, 1000e18 as u128);

                let msg = HandleMsg::Receive {
                    sender: user.clone(),
                    from: user,
                    amount: Uint128(amount),
                    msg: to_binary(&ReceiveMsg::Deposit {}).unwrap(),
                };

                (msg, "eth".to_string())
            }
            "redeem" => {
                let amount: u128 = rng.gen_range(1e12 as u128, 1000e18 as u128);

                let msg = HandleMsg::Redeem {
                    amount: Some(Uint128(amount)),
                };

                (msg, user.0)
            }
            "deadline" if chance == 42 => {
                let config: Config = TypedStore::attach(&deps.storage).load(CONFIG_KEY).unwrap();
                let current = config.deadline as f64;

                let new = rng.gen_range(current + 1.0, current * 1.001);

                let msg = HandleMsg::SetDeadline { block: new as u64 };

                (msg, "admin".to_string())
            }
            "rewards" if chance == 7 => {
                let amount: u128 = rng.gen_range(10000e6 as u128, 100000e6 as u128);

                let msg = HandleMsg::Receive {
                    sender: user.clone(),
                    from: user,
                    amount: Uint128(amount),
                    msg: to_binary(&ReceiveMsg::DepositRewards {}).unwrap(),
                };

                (msg, "scrt".to_string())
            }
            _ => (
                HandleMsg::Redeem {
                    amount: Some(Uint128(u128::MAX)), // This will never work but will keep the tests going
                },
                "".to_string(),
            ),
        }
    }

    fn print_status(
        deps: &Extern<MockStorage, MockApi, MockQuerier>,
        users: Vec<HumanAddr>,
        block: u64,
    ) {
        let reward_pool = TypedStore::<RewardPool, MockStorage>::attach(&deps.storage)
            .load(REWARD_POOL_KEY)
            .unwrap();
        let config: Config = TypedStore::attach(&deps.storage).load(CONFIG_KEY).unwrap();

        println!("####### Statistics for block: {} #######", block);
        println!("Deadline: {}", config.deadline);
        println!("Locked ETH: {}", reward_pool.inc_token_supply);
        println!("Pending rewards: {}", reward_pool.pending_rewards);
        println!(
            "Accumulated rewards per share: {}",
            reward_pool.acc_reward_per_share
        );
        println!("Last reward block: {}", reward_pool.last_reward_block);

        for user in users {
            println!("## {}:", user.0);
            let user_info = TypedStore::<UserInfo, MockStorage>::attach(&deps.storage)
                .load(user.0.as_bytes())
                .unwrap_or(UserInfo { locked: 0, debt: 0 });
            let rewards = query_rewards(deps, user.clone(), block);

            println!("Locked: {}", user_info.locked);
            println!("Debt: {}", user_info.debt);
            println!("Reward: {}", rewards);
        }

        println!();
    }

    fn query_rewards(
        deps: &Extern<MockStorage, MockApi, MockQuerier>,
        user: HumanAddr,
        block: u64,
    ) -> u128 {
        let query_msg = QueryMsg::Rewards {
            address: user,
            height: block,
            key: "42".to_string(),
        };

        let result: QueryAnswer = from_binary(&query(&deps, query_msg).unwrap()).unwrap();
        match result {
            QueryAnswer::Rewards { rewards } => rewards.u128(),
            _ => panic!("NOPE"),
        }
    }

    fn set_vks(deps: &mut Extern<MockStorage, MockApi, MockQuerier>, users: Vec<HumanAddr>) {
        for user in users {
            let vk_msg = SetViewingKey {
                key: "42".to_string(),
                padding: None,
            };
            handle(deps, mock_env(user.0, &[], 2001), vk_msg).unwrap();
        }
    }

    fn extract_rewards(result: StdResult<HandleResponse>) -> u128 {
        match result {
            Ok(resp) => {
                for message in resp.messages {
                    match message {
                        CosmosMsg::Wasm(w) => match w {
                            WasmMsg::Execute {
                                contract_addr, msg, ..
                            } => {
                                if contract_addr == HumanAddr("scrt".to_string()) {
                                    let transfer_msg: Snip20HandleMsg = from_binary(&msg).unwrap();

                                    match transfer_msg {
                                        Snip20HandleMsg::Transfer { amount, .. } => {
                                            return amount.u128();
                                        }
                                        _ => panic!(),
                                    }
                                }
                            }
                            _ => panic!(),
                        },
                        _ => panic!(),
                    }
                }
            }
            Err(e) => match e {
                StdError::NotFound { .. } => {}
                StdError::GenericErr { msg, backtrace } => {
                    if !msg.contains("insufficient") {
                        panic!(format!("{}", msg))
                    }
                }
                _ => panic!(format!("{:?}", e)),
            },
        }

        0
    }

    fn extract_reward_deposit(msg: HandleMsg) -> u128 {
        match msg {
            HandleMsg::Receive { amount, msg, .. } => {
                let transfer_msg: ReceiveMsg = from_binary(&msg).unwrap();

                match transfer_msg {
                    ReceiveMsg::DepositRewards {} => amount.u128(),
                    _ => 0,
                }
            }
            _ => 0,
        }
    }

    fn sanity_run(mut rewards: u128, mut deadline: u64) {
        let mut rng = rand::thread_rng();

        let (init_result, mut deps) = init_helper(deadline);

        deposit_rewards(&mut deps, mock_env("scrt", &[], 1), rewards).unwrap();

        let actions = vec!["deposit", "redeem", "deadline", "rewards"];
        let users = vec![
            HumanAddr("Lebron James".to_string()),
            HumanAddr("Kobe Bryant".to_string()),
            HumanAddr("Giannis".to_string()),
            HumanAddr("Steph Curry".to_string()),
            HumanAddr("Deni Avdija".to_string()),
        ];

        let mut total_rewards_output = 0;

        set_vks(&mut deps, users.clone());
        let mut block: u64 = 2;
        while block < (deadline + 10_000) {
            let num_of_actions = rng.gen_range(0, 5);

            for i in 0..num_of_actions {
                let action_idx = rng.gen_range(0, actions.len());

                let user_idx = rng.gen_range(0, users.len());
                let user = users[user_idx].clone();

                let (msg, sender) = msg_from_action(&deps, actions[action_idx], user.clone());
                rewards += extract_reward_deposit(msg.clone());
                let result = handle(&mut deps, mock_env(sender, &[], block), msg);
                total_rewards_output += extract_rewards(result);
            }

            if block % 10000 == 0 {
                print_status(&deps, users.clone(), block);
            }

            deadline = TypedStore::<Config, MockStorage>::attach(&deps.storage)
                .load(CONFIG_KEY)
                .unwrap()
                .deadline;
            block += 1;
        }

        // Make sure all users are fully redeemed
        for user in users {
            let redeem_msg = HandleMsg::Redeem { amount: None };
            let result = handle(&mut deps, mock_env(user.0, &[], 1_700_000), redeem_msg);
            total_rewards_output += extract_rewards(result);
        }

        let error = 1.0 - (total_rewards_output as f64 / rewards as f64);
        println!("Error is: {}", error);
        assert!(error >= 0f64 && error < 0.01);
    }

    // Tests

    #[test]
    fn test_claim_pool() {
        let (init_result, mut deps) = init_helper(10000000); // Claim height is deadline + 1

        let claim_msg = HandleMsg::ClaimRewardPool { to: None };
        let handle_response = handle(&mut deps, mock_env("not_admin", &[], 10), claim_msg.clone());
        assert_eq!(
            handle_response.unwrap_err(),
            StdError::GenericErr {
                msg: "not an admin: not_admin".to_string(),
                backtrace: None
            }
        );

        let handle_response = handle(&mut deps, mock_env("admin", &[], 10), claim_msg.clone());
        assert_eq!(
            handle_response.unwrap_err(),
            StdError::GenericErr {
                msg: format!("minimum claim height hasn't passed yet: {}", 10000001),
                backtrace: None
            }
        );

        let handle_response = handle(
            &mut deps,
            mock_env("admin", &[], 10000001),
            claim_msg.clone(),
        );
        assert_eq!(
            handle_response.unwrap_err(),
            StdError::GenericErr {
                msg: "Error performing Balance query: Generic error: Querier system error: No such contract: scrt".to_string(), // No way to test external queries yet
                backtrace: None
            }
        );
    }

    #[test]
    fn test_stop_contract() {
        let (init_result, mut deps) = init_helper(10000000);

        let stop_msg = HandleMsg::StopContract {};
        let handle_response = handle(&mut deps, mock_env("not_admin", &[], 10), stop_msg.clone());
        assert_eq!(
            handle_response.unwrap_err(),
            StdError::GenericErr {
                msg: "not an admin: not_admin".to_string(),
                backtrace: None
            }
        );

        let handle_response = handle(&mut deps, mock_env("admin", &[], 10), stop_msg);
        let unwrapped_result: HandleAnswer =
            from_binary(&handle_response.unwrap().data.unwrap()).unwrap();
        assert_eq!(
            to_binary(&unwrapped_result).unwrap(),
            to_binary(&HandleAnswer::StopContract { status: Success }).unwrap()
        );

        let redeem_msg = HandleMsg::Redeem { amount: None };
        let handle_response = handle(&mut deps, mock_env("user", &[], 20), redeem_msg);
        assert_eq!(
            handle_response.unwrap_err(),
            StdError::GenericErr {
                msg: "this contract is stopped and this action is not allowed".to_string(),
                backtrace: None
            }
        );

        let resume_msg = HandleMsg::ResumeContract {};
        let handle_response = handle(&mut deps, mock_env("admin", &[], 21), resume_msg);
        let unwrapped_result: HandleAnswer =
            from_binary(&handle_response.unwrap().data.unwrap()).unwrap();
        assert_eq!(
            to_binary(&unwrapped_result).unwrap(),
            to_binary(&HandleAnswer::ResumeContract { status: Success }).unwrap()
        );

        let redeem_msg = HandleMsg::Redeem { amount: None };
        let handle_response = handle(&mut deps, mock_env("user", &[], 20), redeem_msg);
        let unwrapped_result: HandleAnswer =
            from_binary(&handle_response.unwrap().data.unwrap()).unwrap();
        assert_eq!(
            to_binary(&unwrapped_result).unwrap(),
            to_binary(&HandleAnswer::Redeem { status: Success }).unwrap()
        );
    }

    #[test]
    fn test_admin() {
        let (init_result, mut deps) = init_helper(10000000);

        let admin_action_msg = HandleMsg::ChangeAdmin {
            address: HumanAddr("not_admin".to_string()),
        };
        let handle_response = handle(&mut deps, mock_env("not_admin", &[], 1), admin_action_msg);
        assert_eq!(
            handle_response.unwrap_err(),
            StdError::GenericErr {
                msg: "not an admin: not_admin".to_string(),
                backtrace: None
            }
        );

        let admin_action_msg = HandleMsg::ChangeAdmin {
            address: HumanAddr("new_admin".to_string()),
        };
        let handle_response = handle(&mut deps, mock_env("admin", &[], 1), admin_action_msg);
        let unwrapped_result: HandleAnswer =
            from_binary(&handle_response.unwrap().data.unwrap()).unwrap();
        assert_eq!(
            to_binary(&unwrapped_result).unwrap(),
            to_binary(&HandleAnswer::ChangeAdmin { status: Success }).unwrap()
        );

        let admin_action_msg = HandleMsg::ChangeAdmin {
            address: HumanAddr("not_admin".to_string()),
        };
        let handle_response = handle(&mut deps, mock_env("admin", &[], 1), admin_action_msg);
        assert_eq!(
            handle_response.unwrap_err(),
            StdError::GenericErr {
                msg: "not an admin: admin".to_string(),
                backtrace: None
            }
        );

        let admin_action_msg = HandleMsg::ChangeAdmin {
            address: HumanAddr("not_admin".to_string()),
        };
        let handle_response = handle(&mut deps, mock_env("new_admin", &[], 1), admin_action_msg);
        let unwrapped_result: HandleAnswer =
            from_binary(&handle_response.unwrap().data.unwrap()).unwrap();
        assert_eq!(
            to_binary(&unwrapped_result).unwrap(),
            to_binary(&HandleAnswer::ChangeAdmin { status: Success }).unwrap()
        );
    }

    #[test]
    fn test_single_run() {
        let mut rng = rand::thread_rng();

        let deadline: u64 = rng.gen_range(100_000, 5_000_000);
        let rewards: u128 = rng.gen_range(1_000_000000, 10_000_000_000000); // 1k-10mn SCRT

        sanity_run(rewards, deadline);
    }

    #[test]
    #[ignore]
    fn test_simulations() {
        let mut rng = rand::thread_rng();

        for run in 0..100 {
            let deadline: u64 = rng.gen_range(100_000, 5_000_000);
            let rewards: u128 = rng.gen_range(1_000_000000, 10_000_000_000000); // 1k-10mn SCRT

            println!("$$$$$$$$$$$$$$$$$$ Run Parameters $$$$$$$$$$$$$$$$$$");
            println!("Run number: {}", run + 1);
            println!("Rewards: {}", rewards);
            println!("Deadline: {}", deadline);
            println!();

            sanity_run(rewards, deadline);
        }
    }

    /// SNIP20 token handle messages
    #[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
    #[serde(rename_all = "snake_case")]
    pub enum Snip20HandleMsg {
        // Basic SNIP20 functions
        Transfer {
            recipient: HumanAddr,
            amount: Uint128,
            padding: Option<String>,
        },
    }
}
