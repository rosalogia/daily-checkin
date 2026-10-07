# Build targets used by `sam build` (BuildMethod: makefile). SAM sets ARTIFACTS_DIR.

build-InteractionsFunction:
	cargo lambda build --release --arm64 --bin interactions
	cp target/lambda/interactions/bootstrap $(ARTIFACTS_DIR)/

build-DailyFunction:
	cargo lambda build --release --arm64 --bin daily
	cp target/lambda/daily/bootstrap $(ARTIFACTS_DIR)/
