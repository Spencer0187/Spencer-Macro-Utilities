#pragma once

#include <atomic>
#include <chrono>
#include <mutex>
#include <thread>
#include <vector>

namespace smu::app {

class MacroRuntime {
public:
    MacroRuntime() = default;
    ~MacroRuntime();

    MacroRuntime(const MacroRuntime&) = delete;
    MacroRuntime& operator=(const MacroRuntime&) = delete;

    void start();
    void stop();

private:
    void controllerLoop();
    void refreshTargetProcesses();
    bool foregroundAllows(bool disableOutsideRoblox);
    bool isHotkeyPressed(unsigned int combinedKey) const;
    void processFreezeMacro(bool foregroundAllowed);
    void processPressKeyMacros(bool foregroundAllowed);
    void processWallhopMacros(bool foregroundAllowed);
    void processBunnyhopMacro(bool foregroundAllowed);
    void setTargetSuspended(bool suspended);
    void runWorker(std::thread worker);

    std::atomic<bool> running_{false};
    std::thread controllerThread_;
    mutable std::mutex workerMutex_;
    std::vector<std::thread> workers_;
    std::vector<bool> pressKeyWasPressed_;
    std::vector<bool> wallhopWasPressed_;
    std::vector<unsigned int> frozenPids_;
    std::chrono::steady_clock::time_point nextProcessRefresh_{};
    std::chrono::steady_clock::time_point freezeStartTime_{};
    bool freezeSuspended_ = false;
    bool freezeWasPressed_ = false;
    bool bunnyhopRunning_ = false;
    bool bunnyhopChatLocked_ = false;
};

} // namespace smu::app
