cmake_minimum_required(VERSION 3.24)
project(corsivo_transcription LANGUAGES C CXX)

set(MUSCRIPTOR_BUILD_TESTS OFF CACHE BOOL "" FORCE)
set(MUSCRIPTOR_BUILD_BENCH OFF CACHE BOOL "" FORCE)
add_subdirectory(vendor/cpp muscriptor)

if(TARGET ggml-vulkan)
    find_package(SPIRV-Headers CONFIG REQUIRED)
    target_link_libraries(ggml-vulkan PRIVATE SPIRV-Headers::SPIRV-Headers)
    if(CMAKE_CXX_COMPILER_ID MATCHES "Clang|GNU")
        get_target_property(CORSIVO_VULKAN_DIRECTORY ggml-vulkan SOURCE_DIR)
        set_source_files_properties("${CORSIVO_VULKAN_DIRECTORY}/ggml-vulkan.cpp"
            TARGET_DIRECTORY ggml-vulkan PROPERTIES COMPILE_OPTIONS "-O1")
    endif()
endif()

add_executable(corsivo-transcription bridge.cpp)
target_link_libraries(corsivo-transcription PRIVATE muscriptor_ggml)
target_compile_features(corsivo-transcription PRIVATE cxx_std_23)
set(CORSIVO_ENGINE_OUTPUT "${CMAKE_CURRENT_SOURCE_DIR}/bin" CACHE PATH "Transcription executable directory")
set_target_properties(corsivo-transcription PROPERTIES RUNTIME_OUTPUT_DIRECTORY "${CORSIVO_ENGINE_OUTPUT}")

if(MINGW)
    target_link_options(corsivo-transcription PRIVATE -municode -static -static-libgcc -static-libstdc++)
endif()
