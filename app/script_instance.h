#pragma once

#include "../platform/platform_types.h"

#include <array>
#include <atomic>
#include <chrono>
#include <condition_variable>
#include <filesystem>
#include <cstddef>
#include <mutex>
#include <string>
#include <unordered_map>
#include <unordered_set>
#include <vector>

extern "C" {
#include "lauxlib.h"
#include "lua.h"
}

namespace smu::app {

struct ImportedScriptRecord;

inline constexpr std::size_t kDefaultScriptMemoryLimitMB = 64;
inline constexpr std::size_t kMinScriptMemoryLimitMB = 16;
inline constexpr std::size_t kMaxScriptMemoryLimitMB = 256;
inline constexpr std::size_t kMaxUiIdBytes = 128;
inline constexpr std::size_t kMaxUiControlsPerSettingsCall = 512;
inline constexpr std::size_t kMaxUiStateStringBytes = 4096;
inline constexpr std::size_t kMaxUiStateEntries = 4096;

class ScriptInstance {
public:
    using UiStringBuffer = std::array<char, kMaxUiStateStringBytes + 1>;
    struct SettingsUiControl {
        enum class Kind {
            Text,
            Separator,
            Checkbox,
            SliderInt,
            SliderFloat,
            Textbox,
            DynamicTextbox,
            Keybind
        };

        Kind kind = Kind::Text;
        std::string id;
        std::string label;
        std::string text;
        std::string defaultText;
        bool defaultBool = false;
        int defaultInt = 0;
        int minInt = 0;
        int maxInt = 0;
        double defaultFloat = 0.0;
        double minFloat = 0.0;
        double maxFloat = 0.0;
        unsigned int defaultKeybind = 0;
        float width = 0.0f;
        float height = 0.0f;
    };

    explicit ScriptInstance(ImportedScriptRecord& owner);
    ~ScriptInstance();

    ScriptInstance(const ScriptInstance&) = delete;
    ScriptInstance& operator=(const ScriptInstance&) = delete;

    bool init();
    bool loadFile(const std::filesystem::path& path);
    bool hasFunction(const char* name) const;
    bool hasSettingsCallback() const { return hasSettingsCallback_; }
    bool callOnExecute();
    bool callOnSettings(bool renderMode);
    void renderCachedSettings(bool readOnly);
    void syncSettingsTable();
    void cleanup();
    void* reallocateLuaMemory(void* ptr, std::size_t oldSize, std::size_t newSize);

    ImportedScriptRecord& owner() { return *owner_; }
    const ImportedScriptRecord& owner() const { return *owner_; }
    void setOwner(ImportedScriptRecord& owner) { owner_ = &owner; }
    lua_State* luaState() const { return L_; }

    bool checkDeadline();
    void sleepWithDeadline(std::int64_t ms);
    void scheduleCoroutineSleep(lua_State* thread, std::int64_t ms);
    void requestCancel();
    void throwStopIfRequested(lua_State* L);
    void pauseExecutionBudget();
    bool waitFor(std::chrono::milliseconds duration);
    void setFreeze(bool enabled);
    void setLagSwitch(bool enabled);
    void setSettingsRenderMode(bool enabled) { settingsRenderMode_ = enabled; }
    bool isSettingsRenderMode() const { return settingsRenderMode_; }
    void resetSettingsUiControlCount() { settingsUiControlCount_ = 0; }
    bool tryConsumeSettingsUiControl();
    bool tryRegisterUiId(const std::string& id);
    void syncUiIdCache();
    UiStringBuffer& textboxBuffer(const std::string& id);
    UiStringBuffer& dynamicTextboxBuffer(const std::string& id);
    unsigned int& keybindValue(const std::string& id, unsigned int defaultValue);
    void beginSettingsUiCapture();
    void finishSettingsUiCapture(bool commit);
    void recordSettingsText(std::string text, float width);
    void recordSettingsSeparator(float height);
    void recordSettingsCheckbox(std::string id, std::string label, bool defaultValue, float width);
    void recordSettingsSliderInt(std::string id, std::string label, int defaultValue, int minValue, int maxValue, float width);
    void recordSettingsSliderFloat(std::string id, std::string label, double defaultValue, double minValue, double maxValue, float width);
    void recordSettingsTextbox(std::string id, std::string label, std::string defaultValue, float width, float height);
    void recordSettingsDynamicTextbox(std::string id, std::string label, std::string defaultValue, float width, float height);
    void recordSettingsKeybind(std::string id, std::string label, unsigned int defaultValue, float width);
    bool tryGetTransientUiValue(const std::string& key, std::string& out) const;
    void setTransientUiValue(const std::string& key, std::string value);

private:
    enum class StopReason {
        None,
        Timeout,
        Cancelled
    };

    bool callProtected(int argCount, const char* context);
    void beginTimedCall(std::chrono::steady_clock::duration maxRuntime);
    void beginTimedCall();
    void endTimedCall();
    bool drainSleepingCoroutines();
    bool resumeExecutionBudget();
    std::chrono::steady_clock::time_point computeWakeTime(std::int64_t ms) const;
    bool waitUntil(std::chrono::steady_clock::time_point wakeTime);
    void setStopReason(StopReason reason);
    const char* stopReasonMessage() const;
    void configureMemoryLimit();

    lua_State* L_ = nullptr;
    ImportedScriptRecord* owner_ = nullptr;
    std::chrono::steady_clock::time_point deadline_{};
    std::chrono::steady_clock::duration remainingBudget_{};
    std::chrono::steady_clock::time_point activeBudgetStart_{};
    std::atomic<StopReason> stopReason_{StopReason::None};
    struct SleepingCoroutine {
        lua_State* thread = nullptr;
        int registryRef = LUA_NOREF;
        std::chrono::steady_clock::time_point wakeTime{};
    };
    void releaseSleepingCoroutine(SleepingCoroutine& coroutine);
    void releaseAllSleepingCoroutines();
    std::vector<SleepingCoroutine> sleepingCoroutines_;
    std::vector<smu::platform::PlatformPid> frozenPids_;
    std::unordered_map<std::string, std::string> transientUi_;
    std::unordered_map<std::string, UiStringBuffer> textboxBuffers_;
    std::unordered_map<std::string, UiStringBuffer> dynamicTextboxBuffers_;
    std::unordered_map<std::string, unsigned int> keybindValues_;
    std::vector<SettingsUiControl> settingsUiControls_;
    std::vector<SettingsUiControl> pendingSettingsUiControls_;
    std::unordered_set<std::string> uiIdCache_;
    std::size_t settingsUiControlCount_ = 0;
    std::size_t memoryLimitBytes_ = kDefaultScriptMemoryLimitMB * 1024u * 1024u;
    std::size_t memoryUsedBytes_ = 0;
    bool hasSettingsCallback_ = false;
    bool settingsUiCaptureActive_ = false;
    bool settingsRenderMode_ = false;
    bool budgetActive_ = false;
    std::condition_variable sleepCv_;
    std::mutex sleepMutex_;
    mutable std::mutex settingsUiMutex_;
    mutable std::mutex luaMutex_;
};

ScriptInstance* GetScriptInstance(lua_State* L);

} // namespace smu::app
