#pragma once

#include <atomic>
#include <array>
#include <chrono>
#include <functional>
#include <memory>
#include <mutex>
#include <thread>
#include <vector>

#include "../core/key_codes.h"
#include "../platform/network_backend.h"

namespace smu::platform {
class InputBackend;
}

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
    void refreshTargetProcesses(bool force = false);
    bool foregroundAllows(bool disableOutsideRoblox);
    bool isHotkeyPressed(unsigned int combinedKey) const;
    bool isBunnyhopPhysicallyHeld() const;
    void runBunnyhopWorker();
    void processFreezeMacro(bool foregroundAllowed);
    void processItemDesyncMacro(bool foregroundAllowed);
    void processPressKeyMacros(bool foregroundAllowed);
    void processWallhopMacros(bool foregroundAllowed);
    void processWallessLhjMacro(bool foregroundAllowed);
    void processSpeedglitchMacro(bool foregroundAllowed);
    void processHhjMacro(bool foregroundAllowed);
    void processHhjMotionLoop();
    void processItemUnequipComOffsetMacro(bool foregroundAllowed);
    void processItemClipMacro(bool foregroundAllowed);
    void processLaughClipMacro(bool foregroundAllowed);
    void processWallWalkMacro(bool foregroundAllowed);
    void processSpamKeyMacros();
    void processLedgeBounceMacro(bool foregroundAllowed);
    void processBunnyhopMacro(bool foregroundAllowed);
    void processAntiAfkMacro();
    void processFloorBounceMacro(bool foregroundAllowed);
    void processLagSwitchMacro(bool foregroundAllowed);
    void processImportedScripts();
    void setTargetSuspended(bool suspended);
    std::vector<unsigned int> currentTargetPids();
    void setPidsSuspended(const std::vector<unsigned int>& pids, bool suspended);
    void runWorker(std::function<void()> task);
    void pruneFinishedWorkers();
    void resetInputPollCache();
    bool cachedIsKeyPressed(unsigned int key) const;
    bool isModifierPressed(unsigned int key) const;
    bool areHotkeyModifiersPressed(unsigned int combinedKey) const;
    bool anyInputPressedForHotkeySuppression() const;

    std::atomic<bool> running_{false};
    std::thread controllerThread_;
    mutable std::shared_ptr<smu::platform::InputBackend> inputPollBackend_;
    mutable std::array<unsigned char, smu::core::SMU_VK_MOUSE_WHEEL_DOWN + 1> inputPollKnown_{};
    mutable std::array<unsigned char, smu::core::SMU_VK_MOUSE_WHEEL_DOWN + 1> inputPollPressed_{};
    mutable bool inputPollCacheActive_ = false;
    mutable std::mutex workerMutex_;
    struct WorkerSlot {
        std::thread thread;
        std::atomic<bool> done{false};
    };
    std::vector<std::unique_ptr<WorkerSlot>> workers_;
    std::vector<bool> pressKeyWasPressed_;
    std::vector<bool> wallhopWasPressed_;
    std::vector<bool> spamKeyWasPressed_;
    std::vector<bool> importedScriptWasPressed_;
    std::vector<unsigned int> frozenPids_;
    std::chrono::steady_clock::time_point nextProcessRefresh_{};
    std::chrono::steady_clock::time_point nextForegroundCheck_{};
    std::chrono::steady_clock::time_point freezeStartTime_{};
    smu::platform::LagSwitchConfig lastLagSwitchConfig_{};
    bool cachedForegroundAllowed_ = true;
    bool freezeSuspended_ = false;
    bool freezeWasPressed_ = false;
    bool desyncWasPressed_ = false;
    bool wallessLhjWasPressed_ = false;
    bool speedWasPressed_ = false;
    bool hhjWasPressed_ = false;
    bool itemUnequipWasPressed_ = false;
    bool itemClipWasPressed_ = false;
    bool laughClipWasPressed_ = false;
    bool wallWalkWasPressed_ = false;
    bool ledgeBounceWasPressed_ = false;
    bool floorBounceWasPressed_ = false;
    bool lagSwitchWasPressed_ = false;
    bool hasLastLagSwitchConfig_ = false;
    std::chrono::steady_clock::time_point lagSwitchStartTime_{};
    bool bunnyhopRunning_ = false;
    bool bunnyhopChatLocked_ = false;
    bool antiAfkTimerArmed_ = false;
    std::uint64_t antiAfkLastKeyboardSerialSeen_ = 0;
    std::chrono::steady_clock::time_point antiAfkLastActivity_{};
    std::atomic<bool> bunnyhopWorkerActive_{false};
    std::atomic<bool> itemUnequipWorkerActive_{false};
    std::atomic<bool> laughClipWorkerActive_{false};
};

} // namespace smu::app
