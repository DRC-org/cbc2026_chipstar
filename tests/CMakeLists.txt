cmake_minimum_required(VERSION 3.22)

# ファームウェアの HAL 非依存ロジックを、ホストのコンパイラで検証するための
# テストプロジェクト。各ファームのビルドからは独立している。
project(catchrobo2026_tests CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

set(REPO_ROOT ${CMAKE_CURRENT_SOURCE_DIR}/..)

add_library(doctest INTERFACE)
target_include_directories(doctest INTERFACE ${CMAKE_CURRENT_SOURCE_DIR}/vendor)

enable_testing()

# プロジェクトごとに実行ファイルを分ける。
# インクルードパスを 1 つのファームの src に限定することで、
# HAL や他プロジェクトのヘッダに依存していないことがビルド時に保証される。
function(add_firmware_tests name src_dir)
    add_executable(${name} doctest_main.cpp ${ARGN})
    target_link_libraries(${name} PRIVATE doctest)
    target_include_directories(${name} PRIVATE ${src_dir})
    target_compile_options(${name} PRIVATE -Wall -Wextra)
    add_test(NAME ${name} COMMAND ${name})
endfunction()

add_firmware_tests(serial_svmd_tests ${REPO_ROOT}/serial_svmd/src
    serial_svmd/sts3215_protocol_test.cpp
    ${REPO_ROOT}/serial_svmd/src/domain/sts3215_protocol.cpp
)

add_firmware_tests(cctl_tests ${REPO_ROOT}/cctl/src
    cctl/device_config_test.cpp
    cctl/c620_codec_test.cpp
    cctl/line_reader_test.cpp
    cctl/telemetry_test.cpp
    cctl/command_test.cpp
    cctl/command_queue_test.cpp
    cctl/led_pattern_test.cpp
    cctl/dm_codec_test.cpp
    cctl/el05_codec_test.cpp
    cctl/pid_test.cpp
    cctl/delta_timer_test.cpp
    ${REPO_ROOT}/cctl/src/domain/c620_codec.cpp
    ${REPO_ROOT}/cctl/src/domain/line_reader.cpp
    ${REPO_ROOT}/cctl/src/domain/telemetry.cpp
    ${REPO_ROOT}/cctl/src/domain/command.cpp
    ${REPO_ROOT}/cctl/src/domain/command_queue.cpp
    ${REPO_ROOT}/cctl/src/domain/led_pattern.cpp
    ${REPO_ROOT}/cctl/src/domain/dm_codec.cpp
    ${REPO_ROOT}/cctl/src/domain/el05_codec.cpp
    ${REPO_ROOT}/cctl/src/domain/pid.cpp
    ${REPO_ROOT}/cctl/src/domain/delta_timer.cpp
)
