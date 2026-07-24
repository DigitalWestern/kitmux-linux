cmake_minimum_required(VERSION 3.25)
project(kitmux_linux_proofs VERSION 0.1.0 LANGUAGES C CXX)

set(CMAKE_C_STANDARD 17)
set(CMAKE_C_STANDARD_REQUIRED ON)
set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_POSITION_INDEPENDENT_CODE ON)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

file(REAL_PATH "${CMAKE_CURRENT_SOURCE_DIR}/.." KITMUX_PATH_MAP_ROOT)
add_compile_options(
  "-ffile-prefix-map=${KITMUX_PATH_MAP_ROOT}=."
  "-fmacro-prefix-map=${KITMUX_PATH_MAP_ROOT}=.")

set(KITMUX_REFERENCE_ROOT "${CMAKE_CURRENT_SOURCE_DIR}/../.source/reference"
    CACHE PATH "Materialized tagged macOS reference")
set(LIBKITTY_ROOT "${KITMUX_REFERENCE_ROOT}/libkitty")
set(KITTY_SOURCE_ROOT "${CMAKE_CURRENT_SOURCE_DIR}/../.source/kitty"
    CACHE PATH "Pinned, patched Kitty source")
if(CMAKE_SYSTEM_PROCESSOR MATCHES "^(aarch64|arm64)$")
  set(KITTY_PLATFORM "linux-arm64")
elseif(CMAKE_SYSTEM_PROCESSOR MATCHES "^(x86_64|amd64)$")
  set(KITTY_PLATFORM "linux-64")
else()
  message(FATAL_ERROR
    "Unsupported Linux processor for Kitty dependencies: ${CMAKE_SYSTEM_PROCESSOR}")
endif()
set(KITTY_DEVELOP_ROOT "${KITTY_SOURCE_ROOT}/dependencies/${KITTY_PLATFORM}"
    CACHE PATH "Kitty development dependency bundle")

if(NOT EXISTS "${LIBKITTY_ROOT}/include/libkitty.h")
  message(FATAL_ERROR
    "Locked reference is missing. Run scripts/materialize-reference.sh on the host.")
endif()

find_package(Python3 REQUIRED COMPONENTS Interpreter Development.Embed)

add_library(kitty SHARED
  "${LIBKITTY_ROOT}/src/engine.c"
  "${LIBKITTY_ROOT}/src/session.c"
  "${LIBKITTY_ROOT}/src/render.c")
target_include_directories(kitty
  PUBLIC "${LIBKITTY_ROOT}/include"
  PRIVATE "${LIBKITTY_ROOT}/src")
target_link_libraries(kitty PRIVATE Python3::Python ${CMAKE_DL_LIBS})
target_compile_options(kitty PRIVATE -Wall -Wextra -Wpedantic -UNDEBUG)
target_link_options(kitty PRIVATE
  "-Wl,--version-script=${CMAKE_CURRENT_SOURCE_DIR}/libkitty.map"
  "-Wl,--no-undefined")
set_target_properties(kitty PROPERTIES
  OUTPUT_NAME kitty
  BUILD_RPATH "$ORIGIN"
  INSTALL_RPATH "$ORIGIN")

add_executable(header_c tests/header_c.c)
target_link_libraries(header_c PRIVATE kitty)

add_executable(header_cpp tests/header_cpp.cpp)
target_link_libraries(header_cpp PRIVATE kitty)

add_executable(engine_lifecycle "${LIBKITTY_ROOT}/tests/test_engine.c")
target_link_libraries(engine_lifecycle PRIVATE kitty)
target_compile_options(engine_lifecycle PRIVATE -UNDEBUG)

set(session_test_linux "${CMAKE_CURRENT_BINARY_DIR}/generated/test_session_linux.c")
execute_process(
  COMMAND "${Python3_EXECUTABLE}"
    "${CMAKE_CURRENT_SOURCE_DIR}/scripts/port-session-test.py"
    "${LIBKITTY_ROOT}/tests/test_session.c"
    "${session_test_linux}"
  COMMAND_ERROR_IS_FATAL ANY)

add_executable(session_api "${session_test_linux}")
target_link_libraries(session_api PRIVATE kitty)
target_compile_options(session_api PRIVATE -UNDEBUG)

add_executable(linux_session_stress tests/linux_session_stress.c)
target_link_libraries(linux_session_stress PRIVATE kitty)
target_compile_options(linux_session_stress PRIVATE -UNDEBUG)

enable_testing()
add_test(NAME public_header_c COMMAND header_c)
add_test(NAME public_header_cpp COMMAND header_cpp)
add_test(NAME elf_abi
  COMMAND "${CMAKE_CURRENT_SOURCE_DIR}/scripts/audit-elf.sh" "$<TARGET_FILE:kitty>")
add_test(NAME engine_lifecycle COMMAND engine_lifecycle)
add_test(NAME session_api COMMAND session_api)
add_test(NAME linux_session_stress COMMAND linux_session_stress)
set(libkitty_test_environment
  "KITTY_SRC=${KITTY_SOURCE_ROOT};LIBKITTY_PY=${LIBKITTY_ROOT}/py;LIBKITTY_TEST_CONFIG=${LIBKITTY_ROOT}/tests/fixtures/kitty.conf;PYTHONHOME=${KITTY_DEVELOP_ROOT};LD_LIBRARY_PATH=${KITTY_DEVELOP_ROOT}/lib:$<TARGET_FILE_DIR:kitty>")
set_tests_properties(engine_lifecycle session_api linux_session_stress PROPERTIES
  ENVIRONMENT "${libkitty_test_environment}")
