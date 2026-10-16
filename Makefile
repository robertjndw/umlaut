WASM_TARGET := plugin/target/wasm32-unknown-unknown/release/umlaut_plugin.wasm

# Everything the wasm build needs that is not a Rust crate lives in .tools/, so nothing
# is installed system-wide.
TOOLS := $(CURDIR)/.tools
WASI_SDK_VERSION := 34.0
WASI_SDK := $(TOOLS)/wasi-sdk
WASI_STUB_VERSION := 0.3.1

UNAME_S := $(shell uname -s)
UNAME_M := $(shell uname -m)
ifeq ($(UNAME_S)-$(UNAME_M),Darwin-arm64)
WASI_SDK_PLATFORM := arm64-macos
WASI_SDK_SHA256 := 9c59398106b417f8f14913380fdf0097a8cc0ff4af9eb3ce0065a859e88d49e9
else ifeq ($(UNAME_S)-$(UNAME_M),Darwin-x86_64)
WASI_SDK_PLATFORM := x86_64-macos
WASI_SDK_SHA256 := 87d27fa8adc68dee59bfbf2e22a6d34ef717c34d6bf1d8af2a56fc929d9ce0eb
else ifeq ($(UNAME_S)-$(UNAME_M),Linux-x86_64)
WASI_SDK_PLATFORM := x86_64-linux
WASI_SDK_SHA256 := b761e3a0721dbae9c09a0059e5fdb2bf917d1b4a8a7b430fb3b5aafb0984b2c4
else ifneq ($(filter Linux-aarch64 Linux-arm64,$(UNAME_S)-$(UNAME_M)),)
WASI_SDK_PLATFORM := arm64-linux
WASI_SDK_SHA256 := f7e243dff54d60bcc576e94d6166b69f410f2500ae4a9ceef34315be10e77971
endif
WASI_SDK_URL := https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-$(shell echo $(WASI_SDK_VERSION) | cut -d. -f1)/wasi-sdk-$(WASI_SDK_VERSION)-$(WASI_SDK_PLATFORM).tar.gz

.PHONY: wasm tools test test-plugin test-e2e examples install clean

# Compiling Graphviz for wasm needs wasi-sdk's clang and libc (build.rs picks them up
# from here); the WASI imports that libc brings along are then stubbed out, because
# Typst only provides its own typst_env functions.
export WASI_SDK_PATH := $(WASI_SDK)

# Downloads wasi-sdk and wasi-stub into .tools/. The Graphviz sources themselves are
# fetched by plugin/build.rs into .deps/ on the first cargo build.
tools: $(WASI_SDK)/bin/clang $(TOOLS)/bin/wasi-stub

$(WASI_SDK)/bin/clang:
	@test -n "$(WASI_SDK_PLATFORM)" || { echo "no wasi-sdk build for $(UNAME_S) $(UNAME_M); install wasi-sdk $(WASI_SDK_VERSION) yourself and set WASI_SDK_PATH"; exit 1; }
	mkdir -p $(TOOLS)/cache
	curl -fsSL --retry 3 -o $(TOOLS)/cache/wasi-sdk.tar.gz $(WASI_SDK_URL)
	echo "$(WASI_SDK_SHA256)  $(TOOLS)/cache/wasi-sdk.tar.gz" > $(TOOLS)/cache/wasi-sdk.sha256
	if command -v sha256sum >/dev/null 2>&1; then sha256sum -c $(TOOLS)/cache/wasi-sdk.sha256; else shasum -a 256 -c $(TOOLS)/cache/wasi-sdk.sha256; fi
	rm -rf $(WASI_SDK) $(TOOLS)/wasi-sdk.partial
	mkdir -p $(TOOLS)/wasi-sdk.partial
	tar -xzf $(TOOLS)/cache/wasi-sdk.tar.gz -C $(TOOLS)/wasi-sdk.partial --strip-components=1
	mv $(TOOLS)/wasi-sdk.partial $(WASI_SDK)

$(TOOLS)/bin/wasi-stub:
	cargo install wasi-stub --version $(WASI_STUB_VERSION) --locked --root $(TOOLS)

# Rebuild the plugin. The committed umlaut.wasm must match this output.
# -O3 rather than -Oz: Typst interprets wasm, so speed matters more than a few KB.
wasm: tools
	cargo build --manifest-path plugin/Cargo.toml --release --target wasm32-unknown-unknown
	$(TOOLS)/bin/wasi-stub $(WASM_TARGET) -o $(WASM_TARGET).stubbed
	wasm-opt -O3 --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext \
		--enable-mutable-globals --enable-multivalue --enable-reference-types \
		$(WASM_TARGET).stubbed -o umlaut.wasm

test: wasm test-plugin test-e2e

# Unit tests plus tests that need the typst CLI on PATH (errors).
test-plugin:
	cargo test --manifest-path plugin/Cargo.toml --profile test-opt --lib --test errors

# Every integration test file except errors.rs, so a new format's e2e file runs without
# touching this target.
E2E_TESTS := $(filter-out errors,$(basename $(notdir $(wildcard plugin/tests/*.rs))))

test-e2e:
	cargo test --manifest-path plugin/Cargo.toml --profile test-opt $(E2E_TESTS:%=--test %)

# Every example except the shared helper becomes a picture in docs/ for the README.
EXAMPLES := $(filter-out common,$(basename $(notdir $(wildcard examples/*.typ))))

examples: $(EXAMPLES:%=docs/%.png)

docs/%.png: examples/%.typ umlaut.wasm $(wildcard src/*.typ)
	typst compile --root . --ppi 110 $< $@

# Install the working tree as @local/umlaut:<version>.
install:
	./install.sh

clean:
	cargo clean --manifest-path plugin/Cargo.toml
