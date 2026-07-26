#include "roblox_log_reader.h"

#include <filesystem>
#include <fstream>
#include <iostream>
#include <string>
#include <vector>

namespace {

bool Expect(bool value, const char* message)
{
    if (!value) {
        std::cerr << "FAIL: " << message << '\n';
    }
    return value;
}

} // namespace

int main()
{
    namespace fs = std::filesystem;
    const fs::path directory = fs::temp_directory_path() / "smu-roblox-log-reader-test";
    const fs::path logPath = directory / "Player_test.log";
    std::error_code error;
    fs::remove_all(directory, error);
    fs::create_directories(directory, error);
    if (error) {
        std::cerr << "FAIL: could not create temporary directory\n";
        return 1;
    }

    {
        std::ofstream log(logPath);
        log << "Report game_join_loadtime placeid:123, userid:456, universeid:789,\n";
        log << "Joining game 'test-job'\n";
        log << "existing console message\n";
    }

    smu::platform::RobloxLogReader reader({directory});
    const smu::platform::RobloxLogSnapshot initial = reader.poll(false);
    bool passed = true;
    passed &= Expect(initial.available, "initial log should be available");
    passed &= Expect(initial.startedNewLog, "initial log should be marked new");
    passed &= Expect(initial.lines.empty(), "initial poll should baseline existing lines by default");
    passed &= Expect(initial.state == "in_game", "RoLogParser state should be exposed");
    passed &= Expect(initial.placeId == 123 && initial.userId == 456 && initial.universeId == 789,
                     "RoLogParser IDs should be exposed");
    passed &= Expect(initial.jobId == "test-job", "RoLogParser job ID should be exposed");

    {
        std::ofstream log(logPath, std::ios::app);
        log << "new console message\n";
    }
    const smu::platform::RobloxLogSnapshot appended = reader.poll();
    passed &= Expect(!appended.startedNewLog, "appended log should not be marked new");
    passed &= Expect(appended.lines.size() == 1 && appended.lines.front() == "new console message",
                     "only newly appended raw lines should be returned");

    {
        std::ofstream log(logPath, std::ios::app);
        log << "partial console";
    }
    const smu::platform::RobloxLogSnapshot partial = reader.poll();
    passed &= Expect(partial.lines.empty(), "an unfinished log line should not be returned");
    {
        std::ofstream log(logPath, std::ios::app);
        log << " message\n";
    }
    const smu::platform::RobloxLogSnapshot completed = reader.poll();
    passed &= Expect(completed.lines.size() == 1 && completed.lines.front() == "partial console message",
                     "a completed log line should include its earlier partial content");

    {
        std::ofstream log(logPath, std::ios::trunc);
        log << "rotated console message\n";
    }
    const smu::platform::RobloxLogSnapshot truncated = reader.poll(true);
    passed &= Expect(truncated.startedNewLog, "truncated log should start a new read session");
    passed &= Expect(truncated.lines.size() == 1 && truncated.lines.front() == "rotated console message",
                     "includeExisting should return a new log's existing lines");

    fs::remove_all(directory, error);
    return passed ? 0 : 1;
}
