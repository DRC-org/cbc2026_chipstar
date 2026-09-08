SHELL := /bin/sh
.DEFAULT_GOAL := help

BUILD_TYPE ?= Debug
SERIAL_DEVICE ?= /dev/ttyACM0
BAUD_RATE ?= 115200
PROFILE ?= host/config/rtheta.toml
BOARD ?= network
TEST_SECONDS ?= 5
SOCKET ?=

HOST_SOCKET_ARGS = $(if $(strip $(SOCKET)),--socket "$(SOCKET)")

HOST_MANIFEST := host/Cargo.toml
HOST_BIN := host/target/debug/host
HOSTCTL_BIN := host/target/debug/hostctl
FW_TEST_BIN := host/target/debug/fw_test

CCTL_ELF := cctl/build/$(BUILD_TYPE)/DRC-CCTL2026.elf
DCMD_ELF := dcmd/build/$(BUILD_TYPE)/DRC-DCMD2026.elf
SERIAL_SVMD_ELF := serial_svmd/build/$(BUILD_TYPE)/DRC-SerialSVMD2026.elf

STM32_PROGRAMMER_DEFAULT := $(firstword $(wildcard /opt/st/stm32cubeclt_*/STM32CubeProgrammer/bin/STM32_Programmer_CLI))
STM32_PROGRAMMER ?= $(STM32_PROGRAMMER_DEFAULT)
PIO_DEFAULT := $(firstword $(shell command -v pio 2>/dev/null) $(wildcard $(HOME)/.platformio/penv/bin/pio))
PIO ?= $(PIO_DEFAULT)

.PHONY: help build build-firmware build-host \
	build-cctl build-dcmd build-serial-svmd build-svmd \
	flash-cctl flash-dcmd flash-serial-svmd flash-svmd \
	host host-sim host-sim-headless host-headless status stop estop fw-test \
	test test-host test-firmware \
	check-build-type check-stm32-programmer check-pio check-profile check-host-tools check-fw-board

help:
	@printf '%s\n' \
		'キャチロボ2026 開発コマンド' \
		'' \
		'  make build                  全基板FWとhostをビルド' \
		'  make build-cctl             cctlをビルド' \
		'  make build-dcmd             DCMDをビルド' \
		'  make build-serial-svmd      SerialSVMDをビルド' \
		'  make build-svmd             SVMDをビルド' \
		'' \
		'  make flash-cctl             cctlへ書込み' \
		'  make flash-dcmd             DCMDへ書込み' \
		'  make flash-serial-svmd      SerialSVMDへ書込み' \
		'  make flash-svmd             SVMDへ書込み' \
		'' \
		'  make host                   実機接続でGUIを起動' \
		'  make host-sim               模擬接続でGUIを起動' \
		'  make host-sim-headless      模擬接続でGUIなしhostを起動' \
		'  make host-headless          実機接続でGUIなしhostを起動' \
		'  make status                 起動中hostの状態を表示' \
		'  make stop                   起動中hostを停止・保持' \
		'  make estop                  起動中hostをソフト緊停' \
		'  make fw-test BOARD=network  対話型の基板診断を起動' \
		'  make test                   hostとFWのテストを実行' \
		'' \
		'主な変数:' \
		'  BUILD_TYPE=Debug|Release    STM32のビルド種別（既定: Debug）' \
		'  SERIAL_DEVICE=/dev/ttyACM0  host・診断の接続先' \
		'  BAUD_RATE=115200             シリアル通信速度' \
		'  PROFILE=host/config/...toml  機体プロファイル' \
		'  SOCKET=/path/to/host.sock    host・status・stop・estopの接続先（未指定時はhost既定値）' \
		'  BOARD=network|cctl|svmd|dcmd|serial-svmd' \
		'  TEST_SECONDS=5               診断出力の自動停止秒数（1〜30）'

build: build-firmware build-host

build-firmware: build-cctl build-dcmd build-serial-svmd build-svmd

build-host:
	cargo build --locked --manifest-path "$(HOST_MANIFEST)" --bins

build-cctl: check-build-type
	cmake -S cctl --preset "$(BUILD_TYPE)"
	cmake --build "cctl/build/$(BUILD_TYPE)"

build-dcmd: check-build-type
	cmake -S dcmd --preset "$(BUILD_TYPE)"
	cmake --build "dcmd/build/$(BUILD_TYPE)"

build-serial-svmd: check-build-type
	cmake -S serial_svmd --preset "$(BUILD_TYPE)"
	cmake --build "serial_svmd/build/$(BUILD_TYPE)"

build-svmd: check-pio
	"$(PIO)" run --project-dir svmd --environment uno_r4_minima

flash-cctl: check-stm32-programmer build-cctl
	"$(STM32_PROGRAMMER)" -c port=SWD mode=UR -w "$(CCTL_ELF)" -v -rst

flash-dcmd: check-stm32-programmer build-dcmd
	"$(STM32_PROGRAMMER)" -c port=SWD mode=UR -w "$(DCMD_ELF)" -v -rst

flash-serial-svmd: check-stm32-programmer build-serial-svmd
	"$(STM32_PROGRAMMER)" -c port=SWD mode=UR -w "$(SERIAL_SVMD_ELF)" -v -rst

flash-svmd: check-pio
	"$(PIO)" run --project-dir svmd --environment uno_r4_minima --target upload

host: check-profile build-host
	"$(HOST_BIN)" --serial-device "$(SERIAL_DEVICE)" --baud-rate "$(BAUD_RATE)" --machine-profile "$(PROFILE)" $(HOST_SOCKET_ARGS)

host-sim: check-profile build-host
	"$(HOST_BIN)" --simulate --machine-profile "$(PROFILE)" $(HOST_SOCKET_ARGS)

host-sim-headless: check-profile build-host
	"$(HOST_BIN)" --headless --simulate --machine-profile "$(PROFILE)" $(HOST_SOCKET_ARGS)

host-headless: check-profile build-host
	"$(HOST_BIN)" --headless --serial-device "$(SERIAL_DEVICE)" --baud-rate "$(BAUD_RATE)" --machine-profile "$(PROFILE)" $(HOST_SOCKET_ARGS)

status: check-host-tools
	"$(HOSTCTL_BIN)" status $(HOST_SOCKET_ARGS)

stop: check-host-tools
	"$(HOSTCTL_BIN)" stop $(HOST_SOCKET_ARGS)

estop: check-host-tools
	"$(HOSTCTL_BIN)" estop $(HOST_SOCKET_ARGS)

fw-test: check-fw-board build-host
	"$(FW_TEST_BIN)" --board "$(BOARD)" --serial-device "$(SERIAL_DEVICE)" --baud-rate "$(BAUD_RATE)" --seconds "$(TEST_SECONDS)"

test: test-host test-firmware

test-host:
	cargo test --locked --manifest-path "$(HOST_MANIFEST)"

test-firmware:
	cmake -S tests --preset default
	cmake --build tests/build
	ctest --test-dir tests/build --output-on-failure

check-build-type:
	@case "$(BUILD_TYPE)" in Debug|Release) ;; *) printf 'BUILD_TYPEはDebugまたはReleaseを指定してください。\n' >&2; exit 2;; esac

check-stm32-programmer:
	@test -x "$(STM32_PROGRAMMER)" || { printf 'STM32_Programmer_CLIが見つかりません。STM32_PROGRAMMER=/path/to/STM32_Programmer_CLIを指定してください。\n' >&2; exit 2; }

check-pio:
	@test -x "$(PIO)" || { printf 'PlatformIOが見つかりません。PIO=/path/to/pioを指定してください。\n' >&2; exit 2; }

check-profile:
	@test -f "$(PROFILE)" || { printf '機体プロファイルが見つかりません: %s\n' "$(PROFILE)" >&2; exit 2; }

check-host-tools:
	@test -x "$(HOSTCTL_BIN)" || { printf 'hostctlが未ビルドです。先に make build-host を実行してください。\n' >&2; exit 2; }

check-fw-board:
	@case "$(BOARD)" in network|cctl|svmd|dcmd|serial-svmd) ;; *) printf 'BOARDはnetwork、cctl、svmd、dcmd、serial-svmdのいずれかを指定してください。\n' >&2; exit 2;; esac
