SECRETCLI = docker exec -it secretdev /usr/bin/secretcli

all:
	RUSTFLAGS='-C link-arg=-s' cargo build --release --target wasm32-unknown-unknown
	cp ./target/wasm32-unknown-unknown/release/*.wasm ./contract.wasm
	## The following line is not necessary, may work only on linux (extra size optimization)
	# wasm-opt -Os ./contract.wasm -o ./contract.wasm
	cat ./contract.wasm | gzip -9 > ./contract.wasm.gz

clean:
	cargo clean
	-rm -f ./contract.wasm ./contract.wasm.gz

.PHONY: start-server
start-server: # CTRL+C to stop
	docker run -it --rm \
		-p 26657:26657 -p 26656:26656 -p 1317:1317 \
		-v $$(pwd):/root/code \
		-v $$(pwd)/../secret-secret:/root/secret-secret \
		--name secretdev enigmampc/secret-network-sw-dev:v1.0.4

.PHONY: list-code
list-code:
	$(SECRETCLI) query compute list-code

.PHONY: setup
setup: compile-optimized
	tests/setup.sh

.PHONY: integration-test
integration-test: compile-optimized
	tests/integration.sh

.PHONY: compile _compile
compile: _compile contract.wasm.gz
_compile:
	cargo build --target wasm32-unknown-unknown --locked
	cp ./target/wasm32-unknown-unknown/debug/*.wasm ./contract.wasm

contract.wasm.gz: contract.wasm
	cat ./contract.wasm | gzip -9 > ./contract.wasm.gz

.PHONY: compile-optimized _compile-optimized
compile-optimized: _compile-optimized contract.wasm.gz
_compile-optimized:
	RUSTFLAGS='-C link-arg=-s' cargo build --release --target wasm32-unknown-unknown --locked
	@# The following line is not necessary, may work only on linux (extra size optimization)
	wasm-opt -Os ./target/wasm32-unknown-unknown/release/*.wasm -o ./contract.wasm
