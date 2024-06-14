# Licensed to the Apache Software Foundation (ASF) under one
# or more contributor license agreements.  See the NOTICE file
# distributed with this work for additional information
# regarding copyright ownership.  The ASF licenses this file
# to you under the Apache License, Version 2.0 (the
# "License"); you may not use this file except in compliance
# with the License.  You may obtain a copy of the License at
#
#   http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing,
# software distributed under the License is distributed on an
# "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
# KIND, either express or implied.  See the License for the
# specific language governing permissions and limitations
# under the License.

SHELL = /bin/bash

DIR=$(shell pwd)

.DEFAULT_GOAL := integration-test

init:
	echo "init"
	echo "Git branch: $GITBRANCH"

build-debug:
	ls -alh
	cd $(DIR); cargo build $(CARGO_FEATURE_FLAGS)

build:
	ls -alh
	cd $(DIR); cargo build --release $(CARGO_FEATURE_FLAGS)

build-wal-table-kv:
	ls -alh
	cd $(DIR)/src/horaedb; cargo build --release --no-default-features --features wal-table-kv

build-wal-message-queue:
	ls -alh
	cd $(DIR)/src/horaedb; cargo build --release --no-default-features --features wal-message-queue

build-slim:
	ls -alh
	cd $(DIR); cargo build --profile release-slim $(CARGO_FEATURE_FLAGS)

build-asan:
	ls -alh
	export RUSTFLAGS=-Zsanitizer=address RUSTDOCFLAGS=-Zsanitizer=address
	cd $(DIR); cargo build -Zbuild-std --target x86_64-unknown-linux-gnu --release $(CARGO_FEATURE_FLAGS)

build-arm64:
	ls -alh
	cd $(DIR); cargo build --release --no-default-features $(CARGO_FEATURE_FLAGS)

build-with-console:
	ls -alh
	cd $(DIR); RUSTFLAGS="--cfg tokio_unstable" cargo build --release $(CARGO_FEATURE_FLAGS)

test:
	cd $(DIR); cargo test --workspace -- --test-threads=4

integration-test:
	cd $(DIR)/integration_tests; make run-local
	cd $(DIR)/integration_tests; make run-cluster

# grcov needs build first, then run test
build-ut:
	echo $(CARGO_INCREMENTAL)
	echo $(RUSTFLAGS)
	echo $(RUSTDOCFLAGS)
	cd $(DIR); cargo build --workspace

test-ut:
	echo $(CARGO_INCREMENTAL)
	echo $(RUSTFLAGS)
	echo $(RUSTDOCFLAGS)
	#cd $(DIR); cargo test --workspace -- -Z unstable-options --format json | tee results.json; \
	#cat results.json | cargo2junit > ${WORKSPACE}/testresult/TEST-all.xml
	cargo test --workspace

fmt:
	cd $(DIR); cargo fmt -- --check

check-cargo-toml:
	cd $(DIR); cargo sort --workspace --check

udeps:
	cd $(DIR); cargo udeps --all-targets --all-features --workspace

clippy:
	cd $(DIR); cargo clippy --all-targets --all-features --workspace -- -D warnings -D clippy::dbg-macro

# test with address sanitizer
asan-test:
	export RUSTFLAGS=-Zsanitizer=address RUSTDOCFLAGS=-Zsanitizer=address
	cd $(DIR); cargo test -Zbuild-std --target x86_64-unknown-linux-gnu --workspace

# test with address sanitizer under release mode to workaround `attempt to create unaligned or null slice`
# error in parquet crate.
asan-test-release:
	export RUSTFLAGS=-Zsanitizer=address RUSTDOCFLAGS=-Zsanitizer=address
	cd $(DIR); cargo test -Zbuild-std --target x86_64-unknown-linux-gnu --release --workspace

# test with memory sanitizer
mem-test:
	export RUSTFLAGS=-Zsanitizer=memory RUSTDOCFLAGS=-Zsanitizer=memory
	cd $(DIR); cargo test -Zbuild-std --target x86_64-unknown-linux-gnu --workspace

# test with miri.
# only list packages will be tested.
miri:
	cd $(DIR); cargo miri test --package arena

ensure-disk-quota:
	bash ./scripts/free-disk-space.sh

tsbs: build
	cd $(DIR); sh scripts/run-tsbs.sh

# install dev dependencies
ifeq ($(shell uname), Darwin)
dev-setup:
	echo "Detecting macOS system..."
	brew --version >/dev/null 2>&1 || { echo "Error: Homebrew is not installed. Exiting..."; exit 1; }
	echo "Installing dependencies using Homebrew..."
	HOMEBREW_NO_AUTO_UPDATE=1 brew install git openssl protobuf cmake pre-commit
	cargo install cargo-udeps
	cargo install cargo-sort
else ifeq ($(shell uname), Linux)
dev-setup:
	echo "Detecting Linux system..."
	os_id=$(shell awk -F= '/^ID=/{print $$2}' /etc/os-release) && \
	if [ "$$os_id" = "ubuntu" ]; then \
		echo "Detected Ubuntu system..."; \
		echo "Installing dependencies using apt-get..."; \
		sudo apt-get update; \
		sudo apt install -y git gcc g++ libssl-dev pkg-config protobuf-compiler cmake pre-commit; \
		cargo install cargo-udeps; \
		cargo install cargo-sort; \
	else \
		echo "Error: Unsupported Linux distribution. Exiting..."; \
		exit 1; \
	fi
else
dev-setup:
	echo "Error: Unsupported OS. Exiting..."
	exit 1
endif

fix:
	cargo fmt
	cargo sort --workspace
	cargo clippy --fix --allow-staged --all-targets --all-features --workspace -- -D warnings

update-licenses:
	# Update Horaedb dependencies
	cargo install --locked cargo-deny && cargo deny list -f tsv -l crate > DEPENDENCIES.tsv
	# Update Horaemeta dependencies
	cd $(DIR)/horaemeta; go install github.com/google/go-licenses@latest && go-licenses report ./... > DEPENDENCIES.csv
