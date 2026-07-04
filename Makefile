.PHONY: test cover pbt-with-cover fuzzing fuzzing-parallel fuzzing-list check clippy fmt clean bench compare

# 全テストを実行する
test:
	cargo test --workspace

# 全テストカバレッジ付きで実行する
# (c-api の E2E テストは計測なしでビルドした libflac.a を C からリンクして
# 使うためカバレッジが取れず、除外する)
cover:
	cargo llvm-cov --tests --workspace --exclude c-api

# PBT をカバレッジ付きで実行する
pbt-with-cover:
	cargo llvm-cov -p pbt --tests

# Fuzzing を全ターゲットで逐次実行する（fork 数はコア数に応じて自動調整）
fuzzing:
	@FORKS=$$(( $$(getconf _NPROCESSORS_ONLN) - 2 )); \
	if [ $$FORKS -lt 1 ]; then FORKS=1; fi; \
	echo "Using fork=$$FORKS on $$(getconf _NPROCESSORS_ONLN) cores"; \
	for target in $$(cargo fuzz list); do \
		echo "=== Fuzzing $$target ==="; \
		cargo +nightly fuzz run $$target -- -max_total_time=30 -fork=$$FORKS -max_len=65536 || exit 1; \
	done

# Fuzzing を全ターゲットで並列実行しレポートを出力する
fuzzing-parallel:
	@mkdir -p fuzz/logs
	@cargo fuzz list | xargs -P $$(cargo fuzz list | wc -l) -I {} \
		sh -c 'cargo +nightly fuzz run {} -- -max_total_time=30 -fork=1 -max_len=65536 > fuzz/logs/{}.log 2>&1'
	@echo "=== Fuzzing Report ==="
	@for f in fuzz/logs/*.log; do \
		target=$$(basename $$f .log); \
		last=$$(grep -E '^#[0-9]+:' $$f | tail -1); \
		echo "$$target: $$last"; \
	done

# Fuzzing ターゲット一覧を表示する
fuzzing-list:
	cargo fuzz list

# cargo check を実行する
check:
	cargo check --workspace

# cargo clippy を実行する
clippy:
	cargo clippy --workspace --all-targets -- -D warnings

# cargo fmt を実行する
fmt:
	cargo fmt --all

# ビルド成果物を削除する
clean:
	cargo clean

# ベンチマークを実行する
bench:
	cargo bench -p benches

# 本家 flac コマンドと相互運用・圧縮率・速度を比較する
# 本家 flac は FLAC_BIN 環境変数、次いで PATH から探す
compare:
	cargo build --release -p flac_encode -p flac_decode -p flac_compare
	cargo run --release -p flac_compare
