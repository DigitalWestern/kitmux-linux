cmake_minimum_required(VERSION 3.25)
project(kitmux_linux_proofs VERSION 0.1.0 LANGUAGES C CXX)

option(KITMUX_BUILD_GTK_HOST "Build the Phase 2 GTK/libkitty host" OFF)
option(KITMUX_BUILD_APP "Build the Phase 4 Rust/GTK application" OFF)
option(KITMUX_APP_TEST_HOOKS "Enable the app's test-only modal drivers" OFF)
set(KITMUX_PYTHON_LIBRARY_OVERRIDE "" CACHE FILEPATH
    "Optional isolated libpython used to keep GUI toolkit link paths clean")

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
  PRIVATE "${LIBKITTY_ROOT}/src" ${Python3_INCLUDE_DIRS})
if(KITMUX_PYTHON_LIBRARY_OVERRIDE)
  if(NOT EXISTS "${KITMUX_PYTHON_LIBRARY_OVERRIDE}")
    message(FATAL_ERROR
      "KITMUX_PYTHON_LIBRARY_OVERRIDE does not exist: ${KITMUX_PYTHON_LIBRARY_OVERRIDE}")
  endif()
  target_link_libraries(kitty PRIVATE
    "${KITMUX_PYTHON_LIBRARY_OVERRIDE}" ${CMAKE_DL_LIBS})
else()
  target_link_libraries(kitty PRIVATE Python3::Python ${CMAKE_DL_LIBS})
endif()
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

add_executable(pty_input_recorder tests/pty_input_recorder.c)
target_compile_options(pty_input_recorder PRIVATE
  -Wall -Wextra -Wpedantic -Werror)

if(KITMUX_BUILD_GTK_HOST OR KITMUX_BUILD_APP)
  find_package(PkgConfig REQUIRED)
  pkg_check_modules(GTK4 REQUIRED IMPORTED_TARGET gtk4 epoxy)

  # The GDK -> libkitty key translation carries no display state, so the
  # disposable probe and production app share one implementation.
  add_library(kitmux_key_translation STATIC src/gtk_key_translation.c)
  target_include_directories(kitmux_key_translation PUBLIC src)
  target_link_libraries(kitmux_key_translation PUBLIC kitty PkgConfig::GTK4)
  target_compile_options(kitmux_key_translation PRIVATE
    -Wall -Wextra -Wpedantic -Werror)

  add_library(kitmux_terminal_bridge STATIC src/gtk_terminal_bridge.c)
  target_include_directories(kitmux_terminal_bridge PUBLIC src)
  target_link_libraries(kitmux_terminal_bridge PUBLIC kitty PkgConfig::GTK4)
  target_compile_options(kitmux_terminal_bridge PRIVATE
    -Wall -Wextra -Wpedantic -Werror)

  if(KITMUX_BUILD_GTK_HOST)
    install(TARGETS kitty LIBRARY DESTINATION lib)
    if(KITMUX_PYTHON_LIBRARY_OVERRIDE)
      install(FILES "${KITMUX_PYTHON_LIBRARY_OVERRIDE}" DESTINATION lib)
    endif()
  endif()
  if(KITMUX_BUILD_APP)
    install(TARGETS kitty LIBRARY DESTINATION lib/app)
    if(KITMUX_PYTHON_LIBRARY_OVERRIDE)
      install(FILES "${KITMUX_PYTHON_LIBRARY_OVERRIDE}" DESTINATION lib/app)
    endif()
  endif()
endif()

if(KITMUX_BUILD_GTK_HOST)
  pkg_check_modules(WEBKITGTK REQUIRED IMPORTED_TARGET webkitgtk-6.0)

  add_executable(kitmux_gtk_host src/gtk_terminal_host.c)
  target_link_libraries(kitmux_gtk_host PRIVATE
    kitmux_key_translation PkgConfig::WEBKITGTK m)
  target_compile_options(kitmux_gtk_host PRIVATE
    -Wall -Wextra -Wpedantic -Werror)
  set_target_properties(kitmux_gtk_host PROPERTIES
    INSTALL_RPATH "$ORIGIN/../lib")
  install(TARGETS kitmux_gtk_host RUNTIME DESTINATION bin)

  pkg_check_modules(XTST REQUIRED IMPORTED_TARGET x11 xtst)
  add_executable(x11_key_injector tests/x11_key_injector.c)
  target_link_libraries(x11_key_injector PRIVATE PkgConfig::XTST)
  target_compile_options(x11_key_injector PRIVATE
    -Wall -Wextra -Wpedantic -Werror)

  add_executable(gtk_key_matrix tests/gtk_key_matrix.c)
  target_link_libraries(gtk_key_matrix PRIVATE kitmux_key_translation)
  target_compile_options(gtk_key_matrix PRIVATE
    -Wall -Wextra -Wpedantic -Werror)
endif()

if(KITMUX_BUILD_APP)
  find_program(CARGO_EXECUTABLE cargo REQUIRED)
  set(KITMUX_APP_MANIFEST "${CMAKE_CURRENT_SOURCE_DIR}/rust/app/Cargo.toml")
  set(KITMUX_APP_TARGET_DIR "${CMAKE_CURRENT_BINARY_DIR}/cargo-app")
  set(KITMUX_APP_BINARY "${KITMUX_APP_TARGET_DIR}/release/kitmux")
  set(KITMUX_APP_CARGO_FEATURES "")
  if(KITMUX_APP_TEST_HOOKS)
    set(KITMUX_APP_CARGO_FEATURES --features test-hooks)
  endif()
  add_custom_target(kitmux_app ALL
    COMMAND "${CMAKE_COMMAND}" -E env
      "CARGO_TARGET_DIR=${KITMUX_APP_TARGET_DIR}"
      "KITMUX_NATIVE_LIB_DIR=$<TARGET_FILE_DIR:kitty>"
      "RUSTFLAGS=--remap-path-prefix=${KITMUX_PATH_MAP_ROOT}=."
      "${CARGO_EXECUTABLE}" build --locked --release
        --manifest-path "${KITMUX_APP_MANIFEST}"
        ${KITMUX_APP_CARGO_FEATURES}
    BYPRODUCTS "${KITMUX_APP_BINARY}"
    DEPENDS kitty kitmux_key_translation kitmux_terminal_bridge
    USES_TERMINAL)
  install(PROGRAMS "${KITMUX_APP_BINARY}" DESTINATION bin)
endif()

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
