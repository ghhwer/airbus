cmake_minimum_required(VERSION 3.16)
project(airbus_client VERSION 0.1.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

option(AIRBUS_CLIENT_BUILD_EXAMPLES "Build example binaries" ON)
option(AIRBUS_CLIENT_FETCH_JSON "Fetch nlohmann/json when not found" ON)

file(READ "${CMAKE_CURRENT_SOURCE_DIR}/VERSION" AIRBUS_CLIENT_VERSION)
string(STRIP "${AIRBUS_CLIENT_VERSION}" AIRBUS_CLIENT_VERSION)

add_library(airbus_client
  src/contracts.cpp
  src/endpoint.cpp
  src/listener.cpp
  src/protocol.cpp
  src/queue.cpp
  src/rpc.cpp
  src/transport_posix.cpp
  src/validate.cpp
)
add_library(airbus::client ALIAS airbus_client)

target_include_directories(airbus_client
  PUBLIC
    $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>
    $<INSTALL_INTERFACE:include>
)

find_package(nlohmann_json 3.11 QUIET)
if(nlohmann_json_FOUND)
  target_link_libraries(airbus_client PUBLIC nlohmann_json::nlohmann_json)
elseif(EXISTS "${CMAKE_CURRENT_SOURCE_DIR}/third_party/nlohmann/json.hpp")
  target_include_directories(airbus_client PUBLIC
    $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/third_party>
  )
elseif(AIRBUS_CLIENT_FETCH_JSON)
  include(FetchContent)
  FetchContent_Declare(
    nlohmann_json
    GIT_REPOSITORY https://github.com/nlohmann/json.git
    GIT_TAG v3.11.3
    GIT_SHALLOW TRUE
  )
  FetchContent_MakeAvailable(nlohmann_json)
  target_link_libraries(airbus_client PUBLIC nlohmann_json::nlohmann_json)
else()
  message(FATAL_ERROR
    "nlohmann/json not found. Install it, place third_party/nlohmann/json.hpp, "
    "or enable AIRBUS_CLIENT_FETCH_JSON.")
endif()

find_package(Threads REQUIRED)
target_link_libraries(airbus_client PUBLIC Threads::Threads)

set_target_properties(airbus_client PROPERTIES
  VERSION ${AIRBUS_CLIENT_VERSION}
  SOVERSION 0
  EXPORT_NAME client
)

if(AIRBUS_CLIENT_BUILD_EXAMPLES)
  add_executable(airbus_ping examples/ping.cpp)
  target_link_libraries(airbus_ping PRIVATE airbus::client)
endif()

option(AIRBUS_CLIENT_BUILD_TESTS "Build Catch2 unit/integration tests" OFF)
if(AIRBUS_CLIENT_BUILD_TESTS)
  include(FetchContent)
  FetchContent_Declare(
    Catch2
    GIT_REPOSITORY https://github.com/catchorg/Catch2.git
    GIT_TAG v3.5.4
    GIT_SHALLOW TRUE
  )
  FetchContent_MakeAvailable(Catch2)

  add_executable(airbus_client_tests
    tests/test_endpoint.cpp
    tests/test_protocol.cpp
    tests/test_rpc.cpp
    tests/test_queue.cpp
  )
  target_include_directories(airbus_client_tests PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}/tests
  )
  target_link_libraries(airbus_client_tests PRIVATE airbus::client Catch2::Catch2WithMain)

  include(CTest)
  include(Catch)
  catch_discover_tests(airbus_client_tests
    WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
  )
endif()

include(GNUInstallDirs)
install(TARGETS airbus_client
  ARCHIVE DESTINATION ${CMAKE_INSTALL_LIBDIR}
  LIBRARY DESTINATION ${CMAKE_INSTALL_LIBDIR}
  RUNTIME DESTINATION ${CMAKE_INSTALL_BINDIR}
)
install(DIRECTORY include/ DESTINATION ${CMAKE_INSTALL_INCLUDEDIR})
if(EXISTS "${CMAKE_CURRENT_SOURCE_DIR}/third_party/nlohmann/json.hpp")
  install(DIRECTORY third_party/nlohmann
    DESTINATION ${CMAKE_INSTALL_INCLUDEDIR}
  )
endif()
