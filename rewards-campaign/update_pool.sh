#! /bin/bash

set -e

function create_offline_tx() {
    local pool_addr="$1"
    local pool_hash="$2"
    local admin_addr="$3"
    local signed_tx_file="$4"

    local deadline_file="deadline_tx.json"
    local rewards_file="rewards_tx.json"
    local unsigned_tx_file="unsigned_tx.json"

    echo "Generating offline unsigned tx.."
    secretcli tx compute execute "$pool_addr" '{"key1":"value1"}' --from "$admin_addr" --gas 500000 --generate-only --enclave-key io-master-cert.der --code-hash "$pool_hash" > "$deadline_file"
    secretcli tx compute execute "$pool_addr" '{"key2":"value2"}' --from "$admin_addr" --gas 500000 --generate-only --enclave-key io-master-cert.der --code-hash "$pool_hash" > "$rewards_file"
    jq ".value.msg |= . + $(jq ".value.msg" "$rewards_file")" "$deadline_file" > "$unsigned_tx_file"

    echo "Signing tx..."
    secretcli tx sign "$unsigned_tx_file" --from "$admin_addr" > "$signed_tx_file"
}

function update_pool() {
    local admin_acc_address="$1"

    local pool_label
    local pool_addr
    local pool_hash
    local continue
    local signed_tx
    local signed_tx_file="signed_tx.json"

    echo "Enter pool's label:"
    read pool_label
    pool_addr="$(secretcli q compute label "$pool_label")"
    pool_addr="${pool_addr##* }" # Take the last word of this string
    echo "Pool address is: ""$pool_addr"
    echo "Confirm? [y/n]"
    read continue
    if [ "$continue" != "y" ]; then
        return 0
    fi

    pool_hash="$(secretcli q compute contract-hash "$pool_addr")"
    pool_hash="${pool_hash:2}" # Trim the first 2 chars ('0x')
    echo "Pool hash is: $pool_hash"

    create_offline_tx "$pool_addr" "$pool_hash" "$admin_acc_address" "$signed_tx_file"
    secretcli tx broadcast "$signed_tx_file"
}

function main() {
    local admin_acc_name
    local admin_acc_address
    local another_pool="y"

    echo "### Welcome to the pool updating tool! ###"
    echo ""

    # Query for network parameters to get node's key
    echo "Querying network parameters.."
    secretcli q register secret-network-params

    echo "Enter admin account name:"
    read admin_acc_name
    admin_acc_address="$(secretcli keys show -a "$admin_acc_name")"
    echo "The address of this account is: $admin_acc_address"
    echo ""

    while [ "$another_pool" == "y" ]; do
        update_pool "$admin_acc_address"

        echo "Update another pool? [y/n]"
        read another_pool
    done
}

main
