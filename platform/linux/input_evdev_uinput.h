#pragma once

#include "../input_backend.h"

#include <array>
#include <atomic>
#include <memory>
#include <mutex>
#include <string>
#include <thread>
#include <vector>

namespace smu::platform::linux {

class EvdevUinputInputBackend final : public InputBackend {
public:
    EvdevUinputInputBackend();
    ~EvdevUinputInputBackend() override;

    bool init(std::string* errorMessage = nullptr) override;
    void shutdown() override;

    bool isKeyPressed(PlatformKeyCode key) const override;
    void holdKey(PlatformKeyCode key, bool extended = false) override;
    void releaseKey(PlatformKeyCode key, bool extended = false) override;
    void pressKey(PlatformKeyCode key, int delayMs = 50) override;
    void holdKeyChord(PlatformKeyCode combinedKey) override;
    void releaseKeyChord(PlatformKeyCode combinedKey) override;
    void moveMouse(int dx, int dy) override;
    void mouseWheel(int delta) override;

    std::optional<PlatformKeyCode> getCurrentPressedKey() const override;
    std::string formatKeyName(PlatformKeyCode key) const override;

private:
    void readerThread(std::string devicePath);
    void emit(int type, int code, int value);
    void emitSyn();
    void setKeyStateFromEvdev(unsigned int evdevCode, bool pressed);
    bool setupUinput(std::string* errorMessage);
    bool detectInputDevices(std::string* errorMessage);

    std::atomic<bool> running_{false};
    std::atomic<bool> initialized_{false};
    int uinputFd_ = -1;
    std::vector<std::string> devicePaths_;
    std::vector<std::thread> readerThreads_;
    mutable std::mutex emitMutex_;
    std::array<std::atomic_bool, 258> keyStates_{};
};

std::shared_ptr<InputBackend> CreateEvdevUinputInputBackend();

} // namespace smu::platform::linux
