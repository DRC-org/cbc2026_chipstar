#!/usr/bin/env bash

set -euo pipefail

project_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
ioc_file="${project_dir}/DRC-SerialSVMD2026.ioc"
cubemx_jar=${STM32CUBEMX_JAR:-/opt/st/stm32cubeide_1.19.0/plugins/com.st.stm32cube.common.mx_6.15.0.202507011659/STM32CubeMX.jar}
cubemx_java=${STM32CUBEMX_JAVA:-java}

if [[ ! -r "${cubemx_jar}" ]]; then
    printf 'STM32CubeMX JAR not found: %s\n' "${cubemx_jar}" >&2
    exit 1
fi

if ! command -v "${cubemx_java}" >/dev/null 2>&1; then
    printf 'Java executable not found: %s\n' "${cubemx_java}" >&2
    exit 1
fi

if ! command -v xvfb-run >/dev/null 2>&1; then
    printf 'xvfb-run is required to run STM32CubeMX without a GUI.\n' >&2
    exit 1
fi

printf 'config load "%s"\nproject generate\nexit\n' "${ioc_file}" |
    xvfb-run -a "${cubemx_java}" -jar "${cubemx_jar}" -q /dev/stdin
