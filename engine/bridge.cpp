#include "muscriptor/muscriptor.hpp"

#include <algorithm>
#include <bit>
#include <cstdint>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <limits>
#include <stdexcept>
#include <string>
#include <string_view>
#include <thread>
#include <vector>

std::string json_string(std::string_view value)
{
    std::string result = "\"";
    for (const unsigned char character : value) {
        switch (character) {
        case '\"': result += "\\\""; break;
        case '\\': result += "\\\\"; break;
        case '\n': result += "\\n"; break;
        case '\r': result += "\\r"; break;
        case '\t': result += "\\t"; break;
        default:
            if (character < 32) {
                const char digits[] = "0123456789abcdef";
                result += "\\u00";
                result += digits[character >> 4];
                result += digits[character & 15];
            } else {
                result += character;
            }
        }
    }
    return result + "\"";
}

void progress(float fraction, std::string_view message)
{
    std::cout << "{\"type\":\"progress\",\"progress\":" << fraction
              << ",\"message\":" << json_string(message) << "}" << std::endl;
}

std::uint32_t little_integer(const unsigned char* bytes, std::size_t count)
{
    std::uint32_t result = 0;
    for (std::size_t index = 0; index < count; ++index) {
        result |= static_cast<std::uint32_t>(bytes[index]) << (index * 8);
    }
    return result;
}

std::vector<float> read_audio(const std::filesystem::path& path)
{
    std::ifstream stream(path, std::ios::binary);
    if (!stream) {
        throw std::runtime_error("Cannot open audio input");
    }
    unsigned char header[12];
    if (!stream.read(reinterpret_cast<char*>(header), sizeof(header))
        || std::memcmp(header, "RIFF", 4) != 0 || std::memcmp(header + 8, "WAVE", 4) != 0) {
        throw std::runtime_error("Input must be a mono float32 WAV at 16000 Hz");
    }
    bool valid_format = false;
    std::vector<float> samples;
    unsigned char chunk_header[8];
    while (stream.read(reinterpret_cast<char*>(chunk_header), sizeof(chunk_header))) {
        const auto byte_count = little_integer(chunk_header + 4, 4);
        if (std::memcmp(chunk_header, "fmt ", 4) == 0) {
            if (byte_count < 16 || byte_count > 4096) {
                throw std::runtime_error("Invalid WAV format chunk");
            }
            std::vector<unsigned char> format(byte_count);
            if (!stream.read(reinterpret_cast<char*>(format.data()), byte_count)) {
                throw std::runtime_error("Truncated WAV format chunk");
            }
            const auto encoding = little_integer(format.data(), 2);
            const bool float_encoding = encoding == 3
                || (encoding == 65534 && byte_count >= 40 && little_integer(format.data() + 24, 2) == 3);
            valid_format = float_encoding && little_integer(format.data() + 2, 2) == 1
                && little_integer(format.data() + 4, 4) == 16000
                && little_integer(format.data() + 14, 2) == 32;
        } else if (std::memcmp(chunk_header, "data", 4) == 0) {
            if (!valid_format || byte_count % sizeof(float) != 0) {
                throw std::runtime_error("Input must be a mono float32 WAV at 16000 Hz");
            }
            samples.resize(byte_count / sizeof(float));
            if (!stream.read(reinterpret_cast<char*>(samples.data()), byte_count)) {
                throw std::runtime_error("Truncated WAV audio");
            }
            break;
        } else {
            stream.seekg(byte_count, std::ios::cur);
        }
        if (byte_count & 1) {
            stream.seekg(1, std::ios::cur);
        }
    }
    if (samples.empty()) {
        throw std::runtime_error("Audio input contains no samples");
    }
    return samples;
}

std::filesystem::path utf8_path(const std::string& value)
{
    return std::filesystem::path(std::u8string(value.begin(), value.end()));
}

int run(const std::vector<std::string>& arguments)
{
    try {
        if (arguments.size() < 3) {
            throw std::runtime_error("Usage: corsivo-transcription MODEL AUDIO [INSTRUMENT...]");
        }
        std::cout << std::setprecision(10);
        progress(0.0f, "Lettura audio");
        const auto samples = read_audio(utf8_path(arguments[2]));
        msl::TranscribeOptions options;
        options.n_threads = static_cast<int>(std::max(1u, std::thread::hardware_concurrency() / 2));
        for (std::size_t index = 3; index < arguments.size(); ++index) {
            const std::string_view selected(arguments[index]);
            const auto groups = msl::allInstrumentGroups();
            const auto matching = std::find_if(groups.begin(), groups.end(), [&](const auto group) {
                return msl::instrumentName(group) == selected;
            });
            if (matching == groups.end()) {
                throw std::runtime_error("Unknown instrument: " + std::string(selected));
            }
            options.instruments.push_back(*matching);
        }
        msl::LoadOptions loading;
        loading.on_progress = [](float fraction) { progress(fraction * 0.08f, "Caricamento modello"); };
        auto transcriber = msl::Transcriber::load(utf8_path(arguments[1]), loading);
        if (!transcriber) {
            throw std::runtime_error(std::string(msl::describe(transcriber.error())));
        }
        const auto& device = transcriber->device();
        progress(0.08f, "Trascrizione su " + device.name);
        const auto notes = transcriber->transcribe(samples, options, [&](const msl::TranscriptionUpdate& update) {
            progress(0.08f + update.progress * 0.92f,
                     "Trascritti " + std::to_string(static_cast<int>(update.finalized_through)) + " secondi");
            return true;
        });
        if (!notes) {
            throw std::runtime_error(std::string(msl::describe(notes.error())));
        }
        std::cout << "{\"type\":\"finished\",\"engine\":"
                  << json_string("MuScriptor small · " + device.backend + " · " + device.name) << ",\"notes\":[";
        bool first = true;
        for (const auto& note : *notes) {
            if (!first) {
                std::cout << ',';
            }
            first = false;
            std::cout << "{\"onset\":" << note.onset << ",\"offset\":" << note.offset
                      << ",\"pitch\":" << note.pitch << ",\"program\":" << note.program
                      << ",\"is_drum\":" << (note.is_drum ? "true" : "false") << '}';
        }
        std::cout << "]}" << std::endl;
        return 0;
    } catch (const std::exception& error) {
        std::cout << "{\"type\":\"error\",\"message\":" << json_string(error.what()) << "}" << std::endl;
        return 1;
    }
}

#if defined(_WIN32)
int wmain(int argument_count, wchar_t** arguments)
{
    std::vector<std::string> values;
    for (int index = 0; index < argument_count; ++index) {
        const auto utf8 = std::filesystem::path(arguments[index]).u8string();
        values.emplace_back(utf8.begin(), utf8.end());
    }
    return run(values);
}
#else
int main(int argument_count, char** arguments)
{
    return run(std::vector<std::string>(arguments, arguments + argument_count));
}
#endif
