#include <lua.hpp>

#include <algorithm>
#include <atomic>
#include <chrono>
#include <condition_variable>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <exception>
#include <fstream>
#include <limits>
#include <mutex>
#include <stdexcept>
#include <string>
#include <thread>
#include <vector>

#if defined(_WIN32)
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#elif defined(__APPLE__) || defined(__linux__)
#include <time.h>
#endif

namespace {

using Clock = std::chrono::steady_clock;
using Micros = std::chrono::microseconds;

constexpr std::int64_t kDefaultFirstThresholdUs = 0;
constexpr std::int64_t kDefaultLastThresholdUs = 2'000;
constexpr int kDefaultRepetitions = 12;
constexpr int kDefaultWarmupRepetitions = 3;
constexpr std::int64_t kDefaultP99ErrorUs = 2;

constexpr std::int64_t kSleepDurationsUs[] = {25, 50, 100, 250, 500, 1'000, 2'000};

struct Sample {
    std::int64_t thresholdUs = 0;
    std::int64_t targetUs = 0;
    std::int64_t elapsedUs = 0;
    std::int64_t cpuUs = 0;
};

struct BenchmarkState {
    std::condition_variable sleepCv;
    std::mutex sleepMutex;
    std::int64_t thresholdUs = 0;
    bool recordSamples = false;
    std::vector<Sample> samples;
};

struct Options {
    std::int64_t firstThresholdUs = kDefaultFirstThresholdUs;
    std::int64_t lastThresholdUs = kDefaultLastThresholdUs;
    int repetitions = kDefaultRepetitions;
    int warmupRepetitions = kDefaultWarmupRepetitions;
    std::int64_t p99ErrorUs = kDefaultP99ErrorUs;
    std::string outputPath = "precise_sleep_benchmark.csv";
};

struct ThresholdSummary {
    std::int64_t thresholdUs = 0;
    double meanCpuUs = 0.0;
    std::int64_t p99AbsoluteErrorUs = 0;
    std::int64_t maxAbsoluteErrorUs = 0;
};

std::int64_t ThreadCpuMicros()
{
#if defined(_WIN32)
    FILETIME creationTime{};
    FILETIME exitTime{};
    FILETIME kernelTime{};
    FILETIME userTime{};
    if (!GetThreadTimes(GetCurrentThread(), &creationTime, &exitTime, &kernelTime, &userTime)) {
        return 0;
    }

    ULARGE_INTEGER kernel{};
    kernel.LowPart = kernelTime.dwLowDateTime;
    kernel.HighPart = kernelTime.dwHighDateTime;
    ULARGE_INTEGER user{};
    user.LowPart = userTime.dwLowDateTime;
    user.HighPart = userTime.dwHighDateTime;
    return static_cast<std::int64_t>((kernel.QuadPart + user.QuadPart) / 10);
#elif defined(CLOCK_THREAD_CPUTIME_ID)
    timespec time{};
    if (clock_gettime(CLOCK_THREAD_CPUTIME_ID, &time) == 0) {
        return static_cast<std::int64_t>(time.tv_sec) * 1'000'000 + time.tv_nsec / 1'000;
    }
#endif
    return 0;
}

void CpuRelax()
{
#if defined(_WIN32)
    YieldProcessor();
#elif defined(__x86_64__) || defined(__i386__)
    __builtin_ia32_pause();
#else
    std::atomic_signal_fence(std::memory_order_seq_cst);
#endif
}

void SpinUntil(Clock::time_point wakeTime)
{
    while (Clock::now() < wakeTime) {
        CpuRelax();
    }
}

#if defined(_WIN32)

HANDLE CreateSleepTimer()
{
#ifndef CREATE_WAITABLE_TIMER_HIGH_RESOLUTION
    constexpr DWORD kHighResolutionTimerFlag = 0x00000002;
#else
    constexpr DWORD kHighResolutionTimerFlag = CREATE_WAITABLE_TIMER_HIGH_RESOLUTION;
#endif

    HANDLE timer = CreateWaitableTimerExW(
        nullptr,
        nullptr,
        kHighResolutionTimerFlag,
        TIMER_MODIFY_STATE | SYNCHRONIZE);
    if (!timer) {
        timer = CreateWaitableTimerW(nullptr, FALSE, nullptr);
    }
    return timer;
}

void PreciseWaitWindows(const BenchmarkState& state, Clock::time_point wakeTime)
{
    HANDLE timer = CreateSleepTimer();
    if (!timer) {
        SpinUntil(wakeTime);
        return;
    }

    constexpr LONGLONG kHundredNanosecondsPerSecond = 10'000'000LL;
    constexpr LONGLONG kMaxSingleTimerChunk = 60LL * kHundredNanosecondsPerSecond;

    while (true) {
        const auto now = Clock::now();
        if (now >= wakeTime) {
            break;
        }

        auto remaining = wakeTime - now;
        const auto threshold = Micros(state.thresholdUs);
        if (remaining <= threshold) {
            SpinUntil(wakeTime);
            break;
        }
        remaining -= threshold;

        const auto remainingHundredNs =
            std::chrono::duration_cast<std::chrono::nanoseconds>(remaining).count() / 100;
        const LONGLONG chunkHundredNs = std::clamp<LONGLONG>(
            remainingHundredNs,
            1,
            kMaxSingleTimerChunk);

        LARGE_INTEGER dueTime{};
        dueTime.QuadPart = -chunkHundredNs;
        if (!SetWaitableTimer(timer, &dueTime, 0, nullptr, nullptr, FALSE)) {
            SpinUntil(wakeTime);
            break;
        }
        WaitForSingleObject(timer, INFINITE);
    }

    CloseHandle(timer);
}

#endif

void PreciseWait(BenchmarkState& state, Micros duration)
{
    const auto wakeTime = Clock::now() + duration;

#if defined(_WIN32)
    PreciseWaitWindows(state, wakeTime);
#else
    while (true) {
        const auto now = Clock::now();
        if (now >= wakeTime) {
            return;
        }

        const auto remaining = wakeTime - now;
        const auto threshold = Micros(state.thresholdUs);
        if (remaining <= threshold) {
            SpinUntil(wakeTime);
            return;
        }

        const auto coarseWakeTime = wakeTime - threshold;
        std::unique_lock<std::mutex> lock(state.sleepMutex);
        state.sleepCv.wait_until(lock, coarseWakeTime, [] {
            return false;
        });
    }
#endif
}

int LuaSleepMicros(lua_State* lua)
{
    auto* state = static_cast<BenchmarkState*>(lua_touserdata(lua, lua_upvalueindex(1)));
    const auto targetUs = static_cast<std::int64_t>(luaL_checkinteger(lua, 1));
    const auto wallStart = Clock::now();
    const auto cpuStart = ThreadCpuMicros();

    PreciseWait(*state, Micros(targetUs));

    if (state->recordSamples) {
        const auto elapsedUs = std::chrono::duration_cast<Micros>(Clock::now() - wallStart).count();
        state->samples.push_back(Sample{
            state->thresholdUs,
            targetUs,
            elapsedUs,
            ThreadCpuMicros() - cpuStart,
        });
    }
    return 0;
}

void CheckLua(lua_State* lua, int status)
{
    if (status == LUA_OK) {
        return;
    }

    const char* message = lua_tostring(lua, -1);
    throw std::runtime_error(message ? message : "Lua benchmark failed.");
}

std::int64_t ParseInt64(const char* value, const char* option)
{
    try {
        std::size_t consumed = 0;
        const std::string text(value);
        const auto parsed = std::stoll(text, &consumed);
        if (consumed != text.size()) {
            throw std::invalid_argument("trailing characters");
        }
        return parsed;
    } catch (const std::exception&) {
        throw std::runtime_error(std::string("Invalid value for ") + option + ": " + value);
    }
}

int ParseInt(const char* value, const char* option)
{
    const auto parsed = ParseInt64(value, option);
    if (parsed < std::numeric_limits<int>::min() || parsed > std::numeric_limits<int>::max()) {
        throw std::runtime_error(std::string("Value out of range for ") + option + ": " + value);
    }
    return static_cast<int>(parsed);
}

void PrintUsage(const char* program)
{
    std::fprintf(stderr,
        "Usage: %s [options]\n"
        "  --first-threshold-us N  First threshold to test (default: %lld)\n"
        "  --last-threshold-us N   Last threshold to test (default: %lld)\n"
        "  --repetitions N         Measured repetitions per duration (default: %d)\n"
        "  --warmup-repetitions N  Unrecorded warmups per duration (default: %d)\n"
        "  --p99-error-us N        Maximum accepted p99 absolute error (default: %lld)\n"
        "  --output PATH            CSV output path (default: precise_sleep_benchmark.csv)\n"
        "  --help                   Show this help\n",
        program,
        static_cast<long long>(kDefaultFirstThresholdUs),
        static_cast<long long>(kDefaultLastThresholdUs),
        kDefaultRepetitions,
        kDefaultWarmupRepetitions,
        static_cast<long long>(kDefaultP99ErrorUs));
}

Options ParseOptions(int argc, char** argv)
{
    Options options;
    for (int index = 1; index < argc; ++index) {
        const std::string argument(argv[index]);
        auto requireValue = [&](const char* option) -> const char* {
            if (index + 1 >= argc) {
                throw std::runtime_error(std::string("Missing value for ") + option);
            }
            return argv[++index];
        };

        if (argument == "--first-threshold-us") {
            options.firstThresholdUs = ParseInt64(requireValue(argument.c_str()), argument.c_str());
        } else if (argument == "--last-threshold-us") {
            options.lastThresholdUs = ParseInt64(requireValue(argument.c_str()), argument.c_str());
        } else if (argument == "--repetitions") {
            options.repetitions = ParseInt(requireValue(argument.c_str()), argument.c_str());
        } else if (argument == "--warmup-repetitions") {
            options.warmupRepetitions = ParseInt(requireValue(argument.c_str()), argument.c_str());
        } else if (argument == "--p99-error-us") {
            options.p99ErrorUs = ParseInt64(requireValue(argument.c_str()), argument.c_str());
        } else if (argument == "--output") {
            options.outputPath = requireValue(argument.c_str());
        } else if (argument == "--help" || argument == "-h") {
            PrintUsage(argv[0]);
            std::exit(0);
        } else {
            throw std::runtime_error("Unknown option: " + argument);
        }
    }

    if (options.firstThresholdUs < 0 || options.lastThresholdUs < options.firstThresholdUs) {
        throw std::runtime_error("Threshold range must satisfy 0 <= first <= last.");
    }
    if (options.repetitions <= 0 || options.warmupRepetitions < 0 || options.p99ErrorUs < 0) {
        throw std::runtime_error("Repetitions must be positive and warmups cannot be negative.");
    }
    return options;
}

void RunLuaBenchmark(lua_State* lua, const char* functionName, int repetitions)
{
    lua_getglobal(lua, functionName);
    lua_pushinteger(lua, repetitions);
    CheckLua(lua, lua_pcall(lua, 1, 0, 0));
}

std::int64_t AbsoluteError(const Sample& sample)
{
    const auto error = sample.elapsedUs - sample.targetUs;
    return error < 0 ? -error : error;
}

ThresholdSummary SummarizeThreshold(const std::vector<Sample>& samples)
{
    ThresholdSummary summary;
    if (samples.empty()) {
        return summary;
    }

    summary.thresholdUs = samples.front().thresholdUs;
    std::vector<std::int64_t> absoluteErrors;
    absoluteErrors.reserve(samples.size());
    std::int64_t totalCpuUs = 0;
    for (const auto& sample : samples) {
        const auto absoluteErrorUs = AbsoluteError(sample);
        absoluteErrors.push_back(absoluteErrorUs);
        summary.maxAbsoluteErrorUs = std::max(summary.maxAbsoluteErrorUs, absoluteErrorUs);
        totalCpuUs += sample.cpuUs;
    }

    std::sort(absoluteErrors.begin(), absoluteErrors.end());
    const auto p99Index = std::min(
        absoluteErrors.size() - 1,
        (absoluteErrors.size() * 99) / 100);
    summary.p99AbsoluteErrorUs = absoluteErrors[p99Index];
    summary.meanCpuUs = static_cast<double>(totalCpuUs) / static_cast<double>(samples.size());
    return summary;
}

void PrintRecommendation(const std::vector<ThresholdSummary>& summaries, std::int64_t p99LimitUs)
{
    if (summaries.empty()) {
        return;
    }

    const auto betterCpu = [](const ThresholdSummary& left, const ThresholdSummary& right) {
        if (left.meanCpuUs != right.meanCpuUs) {
            return left.meanCpuUs < right.meanCpuUs;
        }
        return left.thresholdUs < right.thresholdUs;
    };

    const ThresholdSummary* best = nullptr;
    for (const auto& summary : summaries) {
        if (summary.p99AbsoluteErrorUs > p99LimitUs) {
            continue;
        }
        if (!best || betterCpu(summary, *best)) {
            best = &summary;
        }
    }

    if (best) {
        std::fprintf(stderr,
            "Recommended spin threshold: %lld us\n"
            "  mean thread CPU per sleep: %.1f us\n"
            "  p99 absolute timing error: %lld us\n"
            "  maximum absolute timing error: %lld us\n"
            "  selection rule: lowest measured CPU among thresholds with p99 error <= %lld us\n",
            static_cast<long long>(best->thresholdUs),
            best->meanCpuUs,
            static_cast<long long>(best->p99AbsoluteErrorUs),
            static_cast<long long>(best->maxAbsoluteErrorUs),
            static_cast<long long>(p99LimitUs));
        return;
    }

    const auto bestAccuracy = std::min_element(summaries.begin(), summaries.end(),
        [](const ThresholdSummary& left, const ThresholdSummary& right) {
            if (left.p99AbsoluteErrorUs != right.p99AbsoluteErrorUs) {
                return left.p99AbsoluteErrorUs < right.p99AbsoluteErrorUs;
            }
            return left.meanCpuUs < right.meanCpuUs;
        });
    std::fprintf(stderr,
        "No threshold met the p99 error target of %lld us. Best observed accuracy was threshold %lld us (p99 error %lld us, mean CPU %.1f us).\n",
        static_cast<long long>(p99LimitUs),
        static_cast<long long>(bestAccuracy->thresholdUs),
        static_cast<long long>(bestAccuracy->p99AbsoluteErrorUs),
        bestAccuracy->meanCpuUs);
}

} // namespace

int main(int argc, char** argv)
{
    try {
        const Options options = ParseOptions(argc, argv);
        std::ofstream output(options.outputPath, std::ios::trunc);
        if (!output) {
            throw std::runtime_error("Could not open output file: " + options.outputPath);
        }
        output << "threshold_us,target_us,elapsed_us,cpu_us\n";

        BenchmarkState state;
        state.samples.reserve(static_cast<std::size_t>(options.repetitions) * 7);
        std::vector<ThresholdSummary> summaries;
        summaries.reserve(static_cast<std::size_t>(options.lastThresholdUs - options.firstThresholdUs + 1));

        lua_State* lua = luaL_newstate();
        if (!lua) {
            throw std::runtime_error("Could not create Lua state.");
        }
        luaL_openlibs(lua);
        lua_pushlightuserdata(lua, &state);
        lua_pushcclosure(lua, LuaSleepMicros, 1);
        lua_setglobal(lua, "sleepMicros");
        CheckLua(lua, luaL_dostring(lua,
            "function run(repetitions) "
            "  local durations = {25, 50, 100, 250, 500, 1000, 2000} "
            "  for _, us in ipairs(durations) do "
            "    for _ = 1, repetitions do sleepMicros(us) end "
            "  end "
            "end"));

        for (auto thresholdUs = options.firstThresholdUs;
             thresholdUs <= options.lastThresholdUs;
             ++thresholdUs) {
            state.thresholdUs = thresholdUs;
            state.recordSamples = false;
            state.samples.clear();
            RunLuaBenchmark(lua, "run", options.warmupRepetitions);

            state.recordSamples = true;
            RunLuaBenchmark(lua, "run", options.repetitions);
            state.recordSamples = false;

            for (const auto& sample : state.samples) {
                output << sample.thresholdUs << ','
                    << sample.targetUs << ','
                    << sample.elapsedUs << ','
                    << sample.cpuUs << '\n';
            }
            output.flush();
            summaries.push_back(SummarizeThreshold(state.samples));

            if ((thresholdUs - options.firstThresholdUs) % 64 == 0) {
                std::fprintf(stderr, "tested threshold %lld us\n", static_cast<long long>(thresholdUs));
            }
        }

        lua_close(lua);
        std::fprintf(stderr, "Wrote benchmark results to %s\n", options.outputPath.c_str());
        PrintRecommendation(summaries, options.p99ErrorUs);
        return 0;
    } catch (const std::exception& error) {
        std::fprintf(stderr, "precise sleep benchmark: %s\n", error.what());
        return 2;
    }
}
