.PHONY: build deps clips clean-clips

BIN := ./target/release/capcut-cli

build:
	cargo build --release

deps:
	$(BIN) deps check

# Discover trending audio + ranked clips, compose 3 finished MP4s into ./clips.
# Requires TIKTOK_RESEARCH_ACCESS_TOKEN and TWITTER_BEARER_TOKEN in the env.
# Overridable: QUERY, REGION, WINDOW_DAYS, DURATION, RESOLUTION, MIN_LIKES.
clips: build
	./scripts/build-clips.sh

clean-clips:
	rm -rf clips
