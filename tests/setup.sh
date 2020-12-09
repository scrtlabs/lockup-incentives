#!/bin/bash

set -eu
set -o pipefail # If anything in a pipeline fails, the pipe's exit status is a failure
#set -x # Print all commands for debugging

declare -a KEY=(a b c d)

declare -A FROM=(
    [a]='-y --from a'
    [b]='-y --from b'
    [c]='-y --from c'
    [d]='-y --from d'
)

# This means we don't need to configure the cli since it uses the preconfigured cli in the docker.
# We define this as a function rather than as an alias because it has more flexible expansion behavior.
# In particular, it's not possible to dynamically expand aliases, but `tx_of` dynamically executes whatever
# we specify in its arguments.
function secretcli() {
    docker exec secretdev /usr/bin/secretcli "$@"
}

# Just like `echo`, but prints to stderr
function log() {
    echo "$@" >&2
}

# suppress all output to stdout and stderr for the command described in the arguments
function silent() {
    "$@" >/dev/null 2>&1
}

# Pad the string in the first argument to 256 bytes, using spaces
function pad_space() {
    printf '%-256s' "$1"
}

function assert_eq() {
    local left="$1"
    local right="$2"
    local message

    if [[ "$left" != "$right" ]]; then
        if [ -z ${3+x} ]; then
            local lineno="${BASH_LINENO[0]}"
            message="assertion failed on line $lineno - both sides differ. left: ${left@Q}, right: ${right@Q}"
        else
            message="$3"
        fi
        log "$message"
        return 1
    fi

    return 0
}

function assert_ne() {
    local left="$1"
    local right="$2"
    local message

    if [[ "$left" == "$right" ]]; then
        if [ -z ${3+x} ]; then
            local lineno="${BASH_LINENO[0]}"
            message="assertion failed on line $lineno - both sides are equal. left: ${left@Q}, right: ${right@Q}"
        else
            message="$3"
        fi

        log "$message"
        return 1
    fi

    return 0
}

declare -A ADDRESS=(
    [a]="$(secretcli keys show --address a)"
    [b]="$(secretcli keys show --address b)"
    [c]="$(secretcli keys show --address c)"
    [d]="$(secretcli keys show --address d)"
)

declare -A VK=([a]='' [b]='' [c]='' [d]='')

# Generate a label for a contract with a given code id
# This just adds "contract_" before the code id.
function label_by_init_msg() {
    local init_msg="$1"
    local code_id="$2"
    sha256sum <<< "$code_id $init_msg"
}

# Keep polling the blockchain until the tx completes.
# The first argument is the tx hash.
# The second argument is a message that will be logged after every failed attempt.
# The tx information will be returned.
function wait_for_tx() {
    local tx_hash="$1"
    local message="$2"

    local result

    log "waiting on tx: $tx_hash"
    # secretcli will only print to stdout when it succeeds
    until result="$(secretcli query tx "$tx_hash" 2>/dev/null)"; do
        log "$message"
        sleep 1
    done

    # log out-of-gas events
    if jq -e '.raw_log | startswith("execute contract failed: Out of gas: ") or startswith("out of gas:")' <<<"$result" >/dev/null; then
        log "$(jq -r '.raw_log' <<<"$result")"
    fi

    echo "$result"
}

# This is a wrapper around `wait_for_tx` that also decrypts the response,
# and returns a nonzero status code if the tx failed
function wait_for_compute_tx() {
    local tx_hash="$1"
    local message="$2"
    local return_value=0
    local result
    local decrypted

    result="$(wait_for_tx "$tx_hash" "$message")"
    # log "$result"
    if jq -e '.logs == null' <<<"$result" >/dev/null; then
        return_value=1
    fi
    decrypted="$(secretcli query compute tx "$tx_hash")" || return
    log "$decrypted"
    echo "$decrypted"

    return "$return_value"
}

# If the tx failed, return a nonzero status code.
# The decrypted error or message will be echoed
function check_tx() {
    local tx_hash="$1"
    local result
    local return_value=0

    result="$(secretcli query tx "$tx_hash")"
    if jq -e '.logs == null' <<<"$result" >/dev/null; then
        return_value=1
    fi
    decrypted="$(secretcli query compute tx "$tx_hash")" || return
    log "$decrypted"
    echo "$decrypted"

    return "$return_value"
}

# Extract the tx_hash from the output of the command
function tx_of() {
    "$@" | jq -r '.txhash'
}

# Extract the output_data_as_string from the output of the command
function data_of() {
    "$@" | jq -r '.output_data_as_string'
}

function get_generic_err() {
    jq -r '.output_error.generic_err.msg' <<<"$1"
}

# Send a compute transaction and return the tx hash.
# All arguments to this function are passed directly to `secretcli tx compute execute`.
function compute_execute() {
    tx_of secretcli tx compute execute "$@"
}

# Send a query to the contract.
# All arguments to this function are passed directly to `secretcli query compute query`.
function compute_query() {
    secretcli query compute query "$@"
}

function upload_code() {
    local directory="$1"
    local tx_hash
    local code_id

    tx_hash="$(tx_of secretcli tx compute store "code/$directory/contract.wasm.gz" ${FROM[a]} --gas 10000000)"
    code_id="$(
        wait_for_tx "$tx_hash" 'waiting for contract upload' |
            jq -r '.logs[0].events[0].attributes[] | select(.key == "code_id") | .value'
    )"

    log "uploaded contract #$code_id"

    echo "$code_id"
}

function instantiate() {
    local code_id="$1"
    local init_msg="$2"

    log 'sending init message:'
    log "${init_msg@Q}"

    local tx_hash
    tx_hash="$(tx_of secretcli tx compute instantiate "$code_id" "$init_msg" --label "$(label_by_init_msg "$init_msg" "$code_id")" ${FROM[a]} --gas 10000000)"
    wait_for_tx "$tx_hash" 'waiting for init to complete'
}

# This function uploads and instantiates a contract, and returns the new contract's address
function create_contract() {
    local dir="$1"
    local init_msg="$2"

    local code_id
    code_id="$(upload_code "$dir")"

    local init_result
    init_result="$(instantiate "$code_id" "$init_msg")"

    if jq -e '.logs == null' <<<"$init_result" >/dev/null; then
        log "$(secretcli query compute tx "$(jq -r '.txhash' <<<"$init_result")")"
        return 1
    fi

    jq -r '.logs[0].events[0].attributes[] | select(.key == "contract_address") | .value' <<<"$init_result"
}

# This function uploads and instantiates a contract, and returns the new contract's address
function init_contract() {
    local code_id="$1"
    local init_msg="$2"

    local init_result
    init_result="$(instantiate "$code_id" "$init_msg")"

    if jq -e '.logs == null' <<<"$init_result" >/dev/null; then
        log "$(secretcli query compute tx "$(jq -r '.txhash' <<<"$init_result")")"
        return 1
    fi

    jq -r '.logs[0].events[0].attributes[] | select(.key == "contract_address") | .value' <<<"$init_result"
}

function deposit() {
    local contract_addr="$1"
    local key="$2"
    local amount="$3"

    local deposit_message='{"deposit":{"padding":":::::::::::::::::"}}'
    local tx_hash
    local deposit_response
    tx_hash="$(compute_execute "$contract_addr" "$deposit_message" --amount "${amount}uscrt" ${FROM[$key]} --gas 150000)"
    deposit_response="$(data_of wait_for_compute_tx "$tx_hash" "waiting for deposit to \"$key\" to process")"
    assert_eq "$deposit_response" "$(pad_space '{"deposit":{"status":"success"}}')"
    log "deposited ${amount}uscrt to \"$key\" successfully"
}

function get_balance() {
    local contract_addr="$1"
    local address="$2"

    log "querying balance for \"$address\""
    local balance_query='{"balance":{"address":"'"$address"'","key":"123"}}'
    local balance_response
    balance_response="$(compute_query "$contract_addr" "$balance_query")"
    log "balance response was: $balance_response"
    jq -r '.balance.amount' <<<"$balance_response"
}

# Redeem some SCRT from an account
# As you can see, verifying this is happening correctly requires a lot of code
# so I separated it to its own function, because it's used several times.
function redeem() {
    local contract_addr="$1"
    local key="$2"
    local amount="$3"

    local redeem_message
    local tx_hash
    local redeem_tx
    local transfer_attributes
    local redeem_response

    log "redeeming \"$key\""
    redeem_message='{"redeem":{"amount":"'"$amount"'"}}'
    tx_hash="$(compute_execute "$contract_addr" "$redeem_message" ${FROM[$key]} --gas 150000)"
    redeem_tx="$(wait_for_tx "$tx_hash" "waiting for redeem from \"$key\" to process")"
    transfer_attributes="$(jq -r '.logs[0].events[] | select(.type == "transfer") | .attributes' <<<"$redeem_tx")"
    assert_eq "$(jq -r '.[] | select(.key == "recipient") | .value' <<<"$transfer_attributes")" "${ADDRESS[$key]}"
    assert_eq "$(jq -r '.[] | select(.key == "amount") | .value' <<<"$transfer_attributes")" "${amount}uscrt"
    log "redeem response for \"$key\" returned ${amount}uscrt"

    redeem_response="$(data_of check_tx "$tx_hash")"
    assert_eq "$redeem_response" "$(pad_space '{"redeem":{"status":"success"}}')"
    log "redeemed ${amount} from \"$key\" successfully"
}

function get_token_info() {
    local contract_addr="$1"

    local token_info_query='{"token_info":{}}'
    compute_query "$contract_addr" "$token_info_query"
}

function increase_allowance() {
    local contract_addr="$1"
    local owner_key="$2"
    local spender_key="$3"
    local amount="$4"

    local owner_address="${ADDRESS[$owner_key]}"
    local spender_address="${ADDRESS[$spender_key]}"
    local allowance_message='{"increase_allowance":{"spender":"'"$spender_address"'","amount":"'"$amount"'"}}'
    local allowance_response

    tx_hash="$(compute_execute "$contract_addr" "$allowance_message" ${FROM[$owner_key]} --gas 150000)"
    allowance_response="$(data_of wait_for_compute_tx "$tx_hash" "waiting for the increase of \"$spender_key\"'s allowance to \"$owner_key\"'s funds to process")"
    assert_eq "$(jq -r '.increase_allowance.spender' <<<"$allowance_response")" "$spender_address"
    assert_eq "$(jq -r '.increase_allowance.owner' <<<"$allowance_response")" "$owner_address"
    jq -r '.increase_allowance.allowance' <<<"$allowance_response"
    log "Increased allowance given to \"$spender_key\" from \"$owner_key\" by ${amount}uscrt successfully"
}

function decrease_allowance() {
    local contract_addr="$1"
    local owner_key="$2"
    local spender_key="$3"
    local amount="$4"

    local owner_address="${ADDRESS[$owner_key]}"
    local spender_address="${ADDRESS[$spender_key]}"
    local allowance_message='{"decrease_allowance":{"spender":"'"$spender_address"'","amount":"'"$amount"'"}}'
    local allowance_response

    tx_hash="$(compute_execute "$contract_addr" "$allowance_message" ${FROM[$owner_key]} --gas 150000)"
    allowance_response="$(data_of wait_for_compute_tx "$tx_hash" "waiting for the decrease of \"$spender_key\"'s allowance to \"$owner_key\"'s funds to process")"
    assert_eq "$(jq -r '.decrease_allowance.spender' <<<"$allowance_response")" "$spender_address"
    assert_eq "$(jq -r '.decrease_allowance.owner' <<<"$allowance_response")" "$owner_address"
    jq -r '.decrease_allowance.allowance' <<<"$allowance_response"
    log "Decreased allowance given to \"$spender_key\" from \"$owner_key\" by ${amount}uscrt successfully"
}

function get_allowance() {
    local contract_addr="$1"
    local owner_key="$2"
    local spender_key="$3"

    log "querying allowance given to \"$spender_key\" by \"$owner_key\""
    local owner_address="${ADDRESS[$owner_key]}"
    local spender_address="${ADDRESS[$spender_key]}"
    local allowance_query='{"allowance":{"spender":"'"$spender_address"'","owner":"'"$owner_address"'","key":"'"${VK[$owner_key]}"'"}}'
    local allowance_response
    allowance_response="$(compute_query "$contract_addr" "$allowance_query")"
    log "allowance response was: $allowance_response"
    assert_eq "$(jq -r '.allowance.spender' <<<"$allowance_response")" "$spender_address"
    assert_eq "$(jq -r '.allowance.owner' <<<"$allowance_response")" "$owner_address"
    jq -r '.allowance.allowance' <<<"$allowance_response"
}

function log_test_header() {
    log " # Starting ${FUNCNAME[1]}"
}

function main() {
#    log '              <####> Starting integration tests <####>'
    log "secretcli version in the docker image is: $(secretcli version)"

    local prng_seed
    prng_seed="$(base64 <<<'enigma-rocks')"
    local init_msg

    # Store snip20 code
    local code_id
    code_id="$(upload_code '../secret-secret')"

    # secretSCRT init
    init_msg='{"name":"secret-secret","admin":"'"${ADDRESS[a]}"'","symbol":"SSCRT","decimals":6,"initial_balances":[],"prng_seed":"'"$prng_seed"'","config":{"public_total_supply":true}}'
    scrt_contract_addr="$(init_contract "$code_id" "$init_msg")"
#    sscrt_contract_addr="$(create_contract '../secret-secret' "$init_msg")"
    scrt_contract_hash="$(secretcli q compute contract-hash "$scrt_contract_addr")"
    scrt_contract_hash="${scrt_contract_hash:2}"

    # secretETH init
    b_addr="$(secretcli keys show b -a)"
    init_msg='{"name":"secret-eth","admin":"'"${ADDRESS[a]}"'","symbol":"SETH","decimals":18,"initial_balances":[{"address":"'"$b_addr"'", "amount":"1000000000000000000000"}],"prng_seed":"'"$prng_seed"'","config":{"public_total_supply":true}}'
    eth_contract_addr="$(init_contract "$code_id" "$init_msg")"
#    eth_contract_addr="$(create_contract '../secret-secret' "$init_msg")"
    eth_contract_hash="$(secretcli q compute contract-hash "$eth_contract_addr")"
    eth_contract_hash="${eth_contract_hash:2}"

    init_msg='{"reward_token":{"address":"'"$scrt_contract_addr"'", "contract_hash":"'"$scrt_contract_hash"'"}, "inc_token":{"address":"'"$eth_contract_addr"'", "contract_hash":"'"$eth_contract_hash"'"}, "end_by_height":"10000", "pool_claim_block":"100000", "viewing_key": "123", "prng_seed": "MTEK"}'
    lockup_contract_addr="$(create_contract '.' "$init_msg")"
    lockup_contract_hash="$(secretcli q compute contract-hash "$lockup_contract_addr")"
    lockup_contract_hash="${lockup_contract_hash:2}"

    # To make testing faster, check the logs and try to reuse the deployed contract and VKs from previous runs.
    # Remember to comment out the contract deployment and `test_viewing_key` if you do.
#    local contract_addr='secret18vd8fpwxzck93qlwghaj6arh4p7c5n8978vsyg'
#    VK[a]='api_key_Ij3ZwkDOTqMPnmCLGn3F2uX0pMpETw2LTyCkQ0sDMv8='
#    VK[b]='api_key_hV3SlzQMC4YK50GbDrpbjicGOMQpolfPI+O6pMp6oQY='
#    VK[c]='api_key_7Bv00UvQCkZ7SltDn205R0GBugq/l8GnRX6N0JIBQuA='
#    VK[d]='api_key_A3Y3mFe87d2fEq90kNlPSIUSmVgoao448ZpyDAJkB/4='

    # Deposit prize money and transfer to contract
    log 'depositing rewards to secretSCRT and transfer to the lockup contract'
    local rewards='500000000000'
    deposit "$scrt_contract_addr" 'a' "$rewards"

    local receiver_msg='{"add_to_reward_pool":{}}'
    receiver_msg="$(base64 <<<"$receiver_msg")"
    local send_message='{"send":{"recipient":"'"$lockup_contract_addr"'","amount":"'"$rewards"'","msg":"'"$receiver_msg"'"}}'
    local send_response
    tx_hash="$(compute_execute "$scrt_contract_addr" "$send_message" ${FROM[a]} --gas 500000)"
    send_response="$(wait_for_compute_tx "$tx_hash" 'waiting for send from "b" to the lockup to process')"
    log "$send_response"

    balance="$(get_balance "$scrt_contract_addr" "$lockup_contract_addr")"
    log 'lockup contracts scrt balance is: '"$balance"

    local receiver_state_query='{"query_reward_pool_balance":{}}'
    rewards_result="$(compute_query "$lockup_contract_addr" "$receiver_state_query")"
    rewards="$(jq -r '.query_reward_pool_balance.balance' <<<"$rewards_result")"
    log 'lockup contracts rewards pool is: '"$rewards"

    # Lock eth in contract
    log 'locking eth in the lockup contract'
    local amount='100000000000000000000' # 100 eth

    local receiver_msg='{"lock_tokens":{}}'
    receiver_msg="$(base64 <<<"$receiver_msg")"
    local send_message='{"send":{"recipient":"'"$lockup_contract_addr"'","amount":"'"$amount"'","msg":"'"$receiver_msg"'"}}'
    local send_response
    tx_hash="$(compute_execute "$eth_contract_addr" "$send_message" ${FROM[b]} --gas 500000)"
    send_response="$(wait_for_compute_tx "$tx_hash" 'waiting for send from "b" to the lockup to process')"
    log "$send_response"

    log 'setting the viewing key for "b"'
    local set_viewing_key_message='{"set_viewing_key":{"key":"123"}}'
    tx_hash="$(compute_execute "$lockup_contract_addr" "$set_viewing_key_message" ${FROM[b]} --gas 1400000)"
    viewing_key_response="$(data_of wait_for_compute_tx "$tx_hash" 'waiting for viewing key for "b" to be set')"
    assert_eq "$viewing_key_response" "$(pad_space '{"set_viewing_key":{"status":"success"}}')"

    local receiver_msg='{"lock_tokens":{}}'
    receiver_msg="$(base64 <<<"$receiver_msg")"
    local send_message='{"send":{"recipient":"'"$lockup_contract_addr"'","amount":"'"$amount"'","msg":"'"$receiver_msg"'"}}'
    local send_response
    tx_hash="$(compute_execute "$eth_contract_addr" "$send_message" ${FROM[b]} --gas 500000)"
    send_response="$(wait_for_compute_tx "$tx_hash" 'waiting for send from "b" to the lockup to process')"
    log "$send_response"

    local receiver_msg='{"lock_tokens":{}}'
    receiver_msg="$(base64 <<<"$receiver_msg")"
    local send_message='{"send":{"recipient":"'"$lockup_contract_addr"'","amount":"'"$amount"'","msg":"'"$receiver_msg"'"}}'
    local send_response
    tx_hash="$(compute_execute "$eth_contract_addr" "$send_message" ${FROM[b]} --gas 500000)"
    send_response="$(wait_for_compute_tx "$tx_hash" 'waiting for send from "b" to the lockup to process')"
    log "$send_response"

    local receiver_msg='{"lock_tokens":{}}'
    receiver_msg="$(base64 <<<"$receiver_msg")"
    local send_message='{"send":{"recipient":"'"$lockup_contract_addr"'","amount":"'"$amount"'","msg":"'"$receiver_msg"'"}}'
    local send_response
    tx_hash="$(compute_execute "$eth_contract_addr" "$send_message" ${FROM[b]} --gas 500000)"
    send_response="$(wait_for_compute_tx "$tx_hash" 'waiting for send from "b" to the lockup to process')"
    log "$send_response"

    log 'querying rewards for "b"'
    reward_query_b='{"query_rewards":{"address":"'"${ADDRESS[b]}"'","key":"123", "height":"1200"}}'
    result="$(compute_query "$lockup_contract_addr" "$reward_query_b")"
    log "$result"

    log '###### Contracts Details ######'
    log 'code id is: ' "$code_id"
    log ''
    log 'secret addr is: ' "$scrt_contract_addr"
    log 'secret hash is: ' "$scrt_contract_hash"
    log ''
    log 'eth addr is: ' "$eth_contract_addr"
    log 'eth hash is: ' "$eth_contract_hash"
    log ''
    log 'lockup addr is: ' "$lockup_contract_addr"
    log 'lockup hash is: ' "$lockup_contract_hash"
    log ''

    log 'Tests completed successfully'

    '{"name":"secretSCRT","symbol":"SSCRT","decimals":6,"initial_balances":[{"address":"secret1gs8hau7q8xcya2jum7anj9ap47hw96rmhs2smv",""amount":"20000000000000"}],"prng_seed":"MTEK","config":{"public_total_supply":true}}'
    # If everything else worked, return successful status
    return 0
}

main "$@"
