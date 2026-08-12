#include "wayland_screencast.h"

#include "../logging.h"

#include <algorithm>
#include <atomic>
#include <chrono>
#include <condition_variable>
#include <cstdint>
#include <cstring>
#include <memory>
#include <mutex>
#include <string>
#include <utility>
#include <vector>

#if defined(SMU_HAS_WAYLAND_SCREENCAST) && SMU_HAS_WAYLAND_SCREENCAST
#include <dbus/dbus.h>
#include <pipewire/pipewire.h>
#include <spa/buffer/buffer.h>
#include <spa/param/video/format-utils.h>
#include <spa/param/video/raw.h>
#include <spa/utils/result.h>
#include <unistd.h>
#endif

namespace smu::platform::linux {
namespace {

constexpr const char kPortalBusName[] = "org.freedesktop.portal.Desktop";
constexpr const char kPortalObjectPath[] = "/org/freedesktop/portal/desktop";
constexpr const char kScreenCastInterface[] = "org.freedesktop.portal.ScreenCast";
constexpr const char kRequestInterface[] = "org.freedesktop.portal.Request";
constexpr const char kSessionInterface[] = "org.freedesktop.portal.Session";

void SetError(std::string* errorMessage, std::string message)
{
    if (errorMessage) {
        *errorMessage = std::move(message);
    }
}

#if defined(SMU_HAS_WAYLAND_SCREENCAST) && SMU_HAS_WAYLAND_SCREENCAST

std::string DbusErrorMessage(const DBusError& error, const char* fallback)
{
    if (dbus_error_is_set(&error) && error.message) {
        return error.message;
    }
    return fallback;
}

bool OpenDict(DBusMessageIter* parent, DBusMessageIter* dict)
{
    return dbus_message_iter_open_container(parent, DBUS_TYPE_ARRAY, "{sv}", dict) != FALSE;
}

bool CloseDict(DBusMessageIter* parent, DBusMessageIter* dict)
{
    return dbus_message_iter_close_container(parent, dict) != FALSE;
}

bool AppendStringOption(DBusMessageIter* dict, const char* key, const std::string& value)
{
    DBusMessageIter entry;
    DBusMessageIter variant;
    const char* rawValue = value.c_str();
    return dbus_message_iter_open_container(dict, DBUS_TYPE_DICT_ENTRY, nullptr, &entry) != FALSE &&
        dbus_message_iter_append_basic(&entry, DBUS_TYPE_STRING, &key) != FALSE &&
        dbus_message_iter_open_container(&entry, DBUS_TYPE_VARIANT, "s", &variant) != FALSE &&
        dbus_message_iter_append_basic(&variant, DBUS_TYPE_STRING, &rawValue) != FALSE &&
        dbus_message_iter_close_container(&entry, &variant) != FALSE &&
        dbus_message_iter_close_container(dict, &entry) != FALSE;
}

bool AppendUintOption(DBusMessageIter* dict, const char* key, std::uint32_t value)
{
    DBusMessageIter entry;
    DBusMessageIter variant;
    dbus_uint32_t rawValue = value;
    return dbus_message_iter_open_container(dict, DBUS_TYPE_DICT_ENTRY, nullptr, &entry) != FALSE &&
        dbus_message_iter_append_basic(&entry, DBUS_TYPE_STRING, &key) != FALSE &&
        dbus_message_iter_open_container(&entry, DBUS_TYPE_VARIANT, "u", &variant) != FALSE &&
        dbus_message_iter_append_basic(&variant, DBUS_TYPE_UINT32, &rawValue) != FALSE &&
        dbus_message_iter_close_container(&entry, &variant) != FALSE &&
        dbus_message_iter_close_container(dict, &entry) != FALSE;
}

bool AppendBoolOption(DBusMessageIter* dict, const char* key, bool value)
{
    DBusMessageIter entry;
    DBusMessageIter variant;
    dbus_bool_t rawValue = value ? TRUE : FALSE;
    return dbus_message_iter_open_container(dict, DBUS_TYPE_DICT_ENTRY, nullptr, &entry) != FALSE &&
        dbus_message_iter_append_basic(&entry, DBUS_TYPE_STRING, &key) != FALSE &&
        dbus_message_iter_open_container(&entry, DBUS_TYPE_VARIANT, "b", &variant) != FALSE &&
        dbus_message_iter_append_basic(&variant, DBUS_TYPE_BOOLEAN, &rawValue) != FALSE &&
        dbus_message_iter_close_container(&entry, &variant) != FALSE &&
        dbus_message_iter_close_container(dict, &entry) != FALSE;
}

std::string MakeToken(const char* prefix)
{
    static std::atomic<unsigned long long> next{1};
    return std::string(prefix) + "_" + std::to_string(static_cast<unsigned long long>(getpid())) + "_" +
        std::to_string(next.fetch_add(1, std::memory_order_relaxed));
}

DBusMessage* CallPortalMethod(DBusConnection* connection, DBusMessage* request, std::string* errorMessage)
{
    DBusError error;
    dbus_error_init(&error);
    DBusMessage* reply = dbus_connection_send_with_reply_and_block(connection, request, -1, &error);
    dbus_message_unref(request);
    if (!reply) {
        SetError(errorMessage, "ScreenCast portal request failed: " + DbusErrorMessage(error, "no reply"));
        dbus_error_free(&error);
        return nullptr;
    }
    dbus_error_free(&error);
    return reply;
}

bool ReadRequestHandle(DBusMessage* reply, std::string* handle, std::string* errorMessage)
{
    DBusMessageIter iter;
    if (!dbus_message_iter_init(reply, &iter) || dbus_message_iter_get_arg_type(&iter) != DBUS_TYPE_OBJECT_PATH) {
        SetError(errorMessage, "ScreenCast portal returned an invalid request handle.");
        return false;
    }
    const char* rawHandle = nullptr;
    dbus_message_iter_get_basic(&iter, &rawHandle);
    if (!rawHandle || rawHandle[0] == '\0') {
        SetError(errorMessage, "ScreenCast portal returned an empty request handle.");
        return false;
    }
    *handle = rawHandle;
    return true;
}

bool GetStringFromDict(DBusMessageIter* dict, const char* wantedKey, std::string* value)
{
    DBusMessageIter entry;
    dbus_message_iter_recurse(dict, &entry);
    while (dbus_message_iter_get_arg_type(&entry) == DBUS_TYPE_DICT_ENTRY) {
        DBusMessageIter pair;
        dbus_message_iter_recurse(&entry, &pair);
        const char* key = nullptr;
        if (dbus_message_iter_get_arg_type(&pair) == DBUS_TYPE_STRING) {
            dbus_message_iter_get_basic(&pair, &key);
            if (dbus_message_iter_next(&pair) && dbus_message_iter_get_arg_type(&pair) == DBUS_TYPE_VARIANT) {
                DBusMessageIter variant;
                dbus_message_iter_recurse(&pair, &variant);
                if (key && std::strcmp(key, wantedKey) == 0 &&
                    dbus_message_iter_get_arg_type(&variant) == DBUS_TYPE_STRING) {
                    const char* rawValue = nullptr;
                    dbus_message_iter_get_basic(&variant, &rawValue);
                    if (rawValue) {
                        *value = rawValue;
                        return true;
                    }
                }
            }
        }
        dbus_message_iter_next(&entry);
    }
    return false;
}

bool GetStreamNodeFromDict(DBusMessageIter* dict, std::uint32_t* nodeId)
{
    DBusMessageIter entry;
    dbus_message_iter_recurse(dict, &entry);
    while (dbus_message_iter_get_arg_type(&entry) == DBUS_TYPE_DICT_ENTRY) {
        DBusMessageIter pair;
        dbus_message_iter_recurse(&entry, &pair);
        const char* key = nullptr;
        if (dbus_message_iter_get_arg_type(&pair) == DBUS_TYPE_STRING) {
            dbus_message_iter_get_basic(&pair, &key);
            if (dbus_message_iter_next(&pair) && dbus_message_iter_get_arg_type(&pair) == DBUS_TYPE_VARIANT) {
                DBusMessageIter variant;
                dbus_message_iter_recurse(&pair, &variant);
                if (key && std::strcmp(key, "streams") == 0 &&
                    dbus_message_iter_get_arg_type(&variant) == DBUS_TYPE_ARRAY) {
                    DBusMessageIter streams;
                    dbus_message_iter_recurse(&variant, &streams);
                    if (dbus_message_iter_get_arg_type(&streams) != DBUS_TYPE_STRUCT) {
                        return false;
                    }
                    DBusMessageIter stream;
                    dbus_message_iter_recurse(&streams, &stream);
                    if (dbus_message_iter_get_arg_type(&stream) != DBUS_TYPE_UINT32) {
                        return false;
                    }
                    dbus_uint32_t rawNodeId = 0;
                    dbus_message_iter_get_basic(&stream, &rawNodeId);
                    *nodeId = rawNodeId;
                    return rawNodeId != 0;
                }
            }
        }
        dbus_message_iter_next(&entry);
    }
    return false;
}

bool WaitForPortalResponse(
    DBusConnection* connection,
    const std::string& requestPath,
    const char* expectedStringKey,
    std::string* stringValue,
    std::uint32_t* streamNodeId,
    std::string* errorMessage)
{
    const std::string matchRule = "type='signal',interface='" + std::string(kRequestInterface) +
        "',member='Response',path='" + requestPath + "'";
    DBusError matchError;
    dbus_error_init(&matchError);
    dbus_bus_add_match(connection, matchRule.c_str(), &matchError);
    dbus_connection_flush(connection);
    if (dbus_error_is_set(&matchError)) {
        SetError(errorMessage, "Could not wait for the ScreenCast portal response: " +
            DbusErrorMessage(matchError, "D-Bus match registration failed"));
        dbus_error_free(&matchError);
        return false;
    }
    dbus_error_free(&matchError);

    const auto removeMatch = [&] {
        DBusError removeError;
        dbus_error_init(&removeError);
        dbus_bus_remove_match(connection, matchRule.c_str(), &removeError);
        dbus_error_free(&removeError);
    };
    const auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(90);
    while (std::chrono::steady_clock::now() < deadline) {
        if (!dbus_connection_read_write(connection, 250)) {
            SetError(errorMessage, "ScreenCast portal connection closed while waiting for permission.");
            removeMatch();
            return false;
        }

        DBusMessage* message = dbus_connection_pop_message(connection);
        if (!message) {
            continue;
        }

        const bool isResponse = dbus_message_is_signal(message, kRequestInterface, "Response") &&
            requestPath == dbus_message_get_path(message);
        if (!isResponse) {
            dbus_message_unref(message);
            continue;
        }

        DBusMessageIter iter;
        if (!dbus_message_iter_init(message, &iter) || dbus_message_iter_get_arg_type(&iter) != DBUS_TYPE_UINT32) {
            dbus_message_unref(message);
            SetError(errorMessage, "ScreenCast portal returned a malformed permission response.");
            removeMatch();
            return false;
        }
        dbus_uint32_t result = 2;
        dbus_message_iter_get_basic(&iter, &result);
        if (result != 0) {
            dbus_message_unref(message);
            SetError(errorMessage, result == 1
                ? "ScreenCast permission was cancelled."
                : "ScreenCast portal denied or failed the permission request.");
            removeMatch();
            return false;
        }
        if (!dbus_message_iter_next(&iter) || dbus_message_iter_get_arg_type(&iter) != DBUS_TYPE_ARRAY) {
            dbus_message_unref(message);
            SetError(errorMessage, "ScreenCast portal returned no response details.");
            removeMatch();
            return false;
        }

        bool parsed = false;
        if (expectedStringKey && stringValue) {
            parsed = GetStringFromDict(&iter, expectedStringKey, stringValue);
        } else if (streamNodeId) {
            parsed = GetStreamNodeFromDict(&iter, streamNodeId);
        } else {
            parsed = true;
        }
        dbus_message_unref(message);
        if (!parsed) {
            SetError(errorMessage, "ScreenCast portal returned incomplete session details.");
        }
        removeMatch();
        return parsed;
    }

    SetError(errorMessage, "Timed out waiting for the ScreenCast permission dialog.");
    removeMatch();
    return false;
}

bool CallSessionMethod(
    DBusConnection* connection,
    const std::string& method,
    const std::string& sessionHandle,
    std::uint32_t sourceTypes,
    std::string* requestHandle,
    std::string* errorMessage)
{
    DBusMessage* request = dbus_message_new_method_call(
        kPortalBusName, kPortalObjectPath, kScreenCastInterface, method.c_str());
    if (!request) {
        SetError(errorMessage, "Could not allocate a ScreenCast portal request.");
        return false;
    }

    DBusMessageIter args;
    dbus_message_iter_init_append(request, &args);
    const char* rawSessionHandle = sessionHandle.c_str();
    if (dbus_message_iter_append_basic(&args, DBUS_TYPE_OBJECT_PATH, &rawSessionHandle) == FALSE) {
        dbus_message_unref(request);
        SetError(errorMessage, "Could not build a ScreenCast portal request.");
        return false;
    }

    // ScreenCast.Start is (osa{sv}): unlike SelectSources it requires a
    // parent-window identifier between the session object path and options.
    // SMU does not have a stable portal parent handle, so the documented empty
    // string is used and the portal presents its dialog unparented.
    if (method == "Start") {
        const char* parentWindow = "";
        if (dbus_message_iter_append_basic(&args, DBUS_TYPE_STRING, &parentWindow) == FALSE) {
            dbus_message_unref(request);
            SetError(errorMessage, "Could not add the ScreenCast portal parent window.");
            return false;
        }
    }

    DBusMessageIter options;
    if (!OpenDict(&args, &options) ||
        !AppendStringOption(&options, "handle_token", MakeToken("smu_request")) ||
        (method == "SelectSources" &&
            (!AppendUintOption(&options, "types", sourceTypes) ||
             !AppendBoolOption(&options, "multiple", false))) ||
        !CloseDict(&args, &options)) {
        dbus_message_unref(request);
        SetError(errorMessage, "Could not build ScreenCast portal options.");
        return false;
    }

    DBusMessage* reply = CallPortalMethod(connection, request, errorMessage);
    if (!reply) {
        return false;
    }
    const bool ok = ReadRequestHandle(reply, requestHandle, errorMessage);
    dbus_message_unref(reply);
    return ok;
}

bool CreatePortalSession(DBusConnection* connection, std::string* sessionHandle, std::string* errorMessage)
{
    DBusMessage* request = dbus_message_new_method_call(
        kPortalBusName, kPortalObjectPath, kScreenCastInterface, "CreateSession");
    if (!request) {
        SetError(errorMessage, "Could not allocate a ScreenCast portal request.");
        return false;
    }
    DBusMessageIter args;
    DBusMessageIter options;
    dbus_message_iter_init_append(request, &args);
    if (!OpenDict(&args, &options) ||
        !AppendStringOption(&options, "handle_token", MakeToken("smu_request")) ||
        !AppendStringOption(&options, "session_handle_token", MakeToken("smu_session")) ||
        !CloseDict(&args, &options)) {
        dbus_message_unref(request);
        SetError(errorMessage, "Could not build ScreenCast session options.");
        return false;
    }
    DBusMessage* reply = CallPortalMethod(connection, request, errorMessage);
    if (!reply) {
        return false;
    }
    std::string requestHandle;
    const bool gotHandle = ReadRequestHandle(reply, &requestHandle, errorMessage);
    dbus_message_unref(reply);
    return gotHandle && WaitForPortalResponse(
        connection, requestHandle, "session_handle", sessionHandle, nullptr, errorMessage);
}

int OpenPipeWireRemote(DBusConnection* connection, const std::string& sessionHandle, std::string* errorMessage)
{
    DBusMessage* request = dbus_message_new_method_call(
        kPortalBusName, kPortalObjectPath, kScreenCastInterface, "OpenPipeWireRemote");
    if (!request) {
        SetError(errorMessage, "Could not allocate the ScreenCast PipeWire request.");
        return -1;
    }
    DBusMessageIter args;
    DBusMessageIter options;
    dbus_message_iter_init_append(request, &args);
    const char* rawSessionHandle = sessionHandle.c_str();
    if (dbus_message_iter_append_basic(&args, DBUS_TYPE_OBJECT_PATH, &rawSessionHandle) == FALSE ||
        !OpenDict(&args, &options) || !CloseDict(&args, &options)) {
        dbus_message_unref(request);
        SetError(errorMessage, "Could not build the ScreenCast PipeWire request.");
        return -1;
    }
    DBusMessage* reply = CallPortalMethod(connection, request, errorMessage);
    if (!reply) {
        return -1;
    }
    DBusMessageIter iter;
    if (!dbus_message_iter_init(reply, &iter) || dbus_message_iter_get_arg_type(&iter) != DBUS_TYPE_UNIX_FD) {
        dbus_message_unref(reply);
        SetError(errorMessage, "ScreenCast portal did not provide a PipeWire connection.");
        return -1;
    }
    int fd = -1;
    dbus_message_iter_get_basic(&iter, &fd);
    dbus_message_unref(reply);
    if (fd < 0) {
        SetError(errorMessage, "ScreenCast portal returned an invalid PipeWire connection.");
    }
    return fd;
}

#endif

} // namespace

class WaylandScreenCast::Impl {
public:
    bool supported() const
    {
#if defined(SMU_HAS_WAYLAND_SCREENCAST) && SMU_HAS_WAYLAND_SCREENCAST
        return true;
#else
        return false;
#endif
    }

    bool active() const
    {
        std::lock_guard<std::mutex> lock(mutex_);
        return active_;
    }

    std::string status() const
    {
        std::lock_guard<std::mutex> lock(mutex_);
        return status_;
    }

    std::optional<ScreenBounds> bounds() const
    {
        std::lock_guard<std::mutex> lock(mutex_);
        if (frameWidth_ <= 0 || frameHeight_ <= 0) {
            return std::nullopt;
        }
        return ScreenBounds{0, 0, frameWidth_, frameHeight_};
    }

    std::optional<PixelColor> sample(int x, int y, std::string* errorMessage) const
    {
        std::lock_guard<std::mutex> lock(mutex_);
        if (!active_) {
            SetError(errorMessage, "Wayland screen reads require a ScreenCast session. Open Settings and choose Enable Wayland Screen Capture.");
            return std::nullopt;
        }
        if (frameWidth_ <= 0 || frameHeight_ <= 0 || frame_.empty()) {
            SetError(errorMessage, "Wayland ScreenCast is active but has not delivered a frame yet.");
            return std::nullopt;
        }
        if (x < 0 || y < 0 || x >= frameWidth_ || y >= frameHeight_) {
            SetError(errorMessage, "requested pixel is outside the captured Wayland monitor.");
            return std::nullopt;
        }
        const std::size_t offset = (static_cast<std::size_t>(y) * static_cast<std::size_t>(frameWidth_) +
            static_cast<std::size_t>(x)) * 3U;
        return PixelColor{frame_[offset], frame_[offset + 1], frame_[offset + 2]};
    }

    bool start(std::string* errorMessage)
    {
#if !defined(SMU_HAS_WAYLAND_SCREENCAST) || !SMU_HAS_WAYLAND_SCREENCAST
        SetError(errorMessage, "This SMU build was compiled without PipeWire ScreenCast support.");
        return false;
#else
        stop();
        {
            std::lock_guard<std::mutex> lock(mutex_);
            status_ = "Requesting monitor access from the desktop portal...";
        }

        DBusError dbusError;
        dbus_error_init(&dbusError);
        DBusConnection* connection = dbus_bus_get_private(DBUS_BUS_SESSION, &dbusError);
        if (!connection) {
            const std::string reason = DbusErrorMessage(dbusError, "session bus is unavailable");
            dbus_error_free(&dbusError);
            SetError(errorMessage, "Wayland ScreenCast requires the desktop session D-Bus: " + reason);
            return false;
        }
        dbus_error_free(&dbusError);
        dbus_connection_set_exit_on_disconnect(connection, FALSE);

        // Install this before sending the first asynchronous portal request.
        // A backend is allowed to emit Response before the method reply reaches
        // us, so adding a request-specific match only after that reply races
        // with an immediate response.
        DBusError responseMatchError;
        dbus_error_init(&responseMatchError);
        dbus_bus_add_match(connection,
            "type='signal',interface='org.freedesktop.portal.Request',member='Response'",
            &responseMatchError);
        dbus_connection_flush(connection);
        if (dbus_error_is_set(&responseMatchError)) {
            const std::string reason = DbusErrorMessage(responseMatchError, "D-Bus match registration failed");
            dbus_error_free(&responseMatchError);
            dbus_connection_close(connection);
            dbus_connection_unref(connection);
            SetError(errorMessage, "Could not subscribe to ScreenCast portal responses: " + reason);
            return false;
        }
        dbus_error_free(&responseMatchError);

        std::string sessionHandle;
        std::string portalError;
        std::uint32_t nodeId = 0;
        bool ok = CreatePortalSession(connection, &sessionHandle, &portalError);
        std::string requestHandle;
        if (ok) {
            ok = CallSessionMethod(connection, "SelectSources", sessionHandle, 1, &requestHandle, &portalError) &&
                WaitForPortalResponse(connection, requestHandle, nullptr, nullptr, nullptr, &portalError);
        }
        if (ok) {
            ok = CallSessionMethod(connection, "Start", sessionHandle, 0, &requestHandle, &portalError) &&
                WaitForPortalResponse(connection, requestHandle, nullptr, nullptr, &nodeId, &portalError);
        }
        const int pipeWireFd = ok ? OpenPipeWireRemote(connection, sessionHandle, &portalError) : -1;
        if (!ok || pipeWireFd < 0 || !startPipeWire(pipeWireFd, nodeId, &portalError)) {
            CloseSession(connection, sessionHandle);
            dbus_connection_close(connection);
            dbus_connection_unref(connection);
            SetError(errorMessage, portalError.empty() ? "Could not start Wayland ScreenCast." : portalError);
            std::lock_guard<std::mutex> lock(mutex_);
            status_ = errorMessage ? *errorMessage : "Wayland ScreenCast could not start.";
            return false;
        }

        {
            std::unique_lock<std::mutex> lock(mutex_);
            portalConnection_ = connection;
            sessionHandle_ = std::move(sessionHandle);
            active_ = true;
            status_ = "Wayland ScreenCast is active. Pixel APIs target the monitor selected in the portal.";

            // The portal's Start response only means that PipeWire was set up.
            // A script must not resume until there is an actual frame to read.
            const bool receivedFrame = frameCv_.wait_for(lock, std::chrono::seconds(5), [this] {
                return (frameWidth_ > 0 && frameHeight_ > 0 && !frame_.empty()) || !active_;
            });
            if (!receivedFrame || !active_ || frame_.empty()) {
                const std::string failure = !active_ ? status_
                    : "Wayland ScreenCast did not deliver a frame within 5 seconds.";
                lock.unlock();
                stop();
                SetError(errorMessage, failure);
                return false;
            }
        }
        LogInfo("Wayland ScreenCast session started.");
        return true;
#endif
    }

    void stop()
    {
#if defined(SMU_HAS_WAYLAND_SCREENCAST) && SMU_HAS_WAYLAND_SCREENCAST
        DBusConnection* connection = nullptr;
        std::string sessionHandle;
        {
            std::lock_guard<std::mutex> lock(mutex_);
            connection = portalConnection_;
            portalConnection_ = nullptr;
            sessionHandle = std::move(sessionHandle_);
            active_ = false;
            frame_.clear();
            frameWidth_ = 0;
            frameHeight_ = 0;
            status_ = "Wayland ScreenCast is inactive.";
        }
        frameCv_.notify_all();
        stopPipeWire();
        if (connection) {
            CloseSession(connection, sessionHandle);
            dbus_connection_close(connection);
            dbus_connection_unref(connection);
        }
#endif
    }

    bool requestActivationForScript(const std::function<bool()>& isCancelled, std::string* errorMessage)
    {
        if (active()) {
            return true;
        }
        if (!supported()) {
            SetError(errorMessage, "This SMU build was compiled without PipeWire ScreenCast support.");
            return false;
        }

        std::unique_lock<std::mutex> lock(activationMutex_);
        if (active()) {
            return true;
        }
        if (activationState_ == ActivationState::Idle) {
            activationState_ = ActivationState::AwaitingUser;
            ++activationGeneration_;
        }
        const unsigned long long generation = activationGeneration_;
        while (activationCompletedGeneration_ < generation) {
            if (isCancelled && isCancelled()) {
                SetError(errorMessage, "script was stopped while waiting for Wayland screen-capture permission");
                return false;
            }
            activationCv_.wait_for(lock, std::chrono::milliseconds(100));
        }
        if (activationSucceeded_) {
            return true;
        }
        SetError(errorMessage, activationError_.empty()
                ? "Wayland screen capture was declined."
                : activationError_);
        return false;
    }

    bool hasPendingActivationRequest() const
    {
        std::lock_guard<std::mutex> lock(activationMutex_);
        return activationState_ == ActivationState::AwaitingUser;
    }

    void approveActivationRequest()
    {
        unsigned long long generation = 0;
        {
            std::lock_guard<std::mutex> lock(activationMutex_);
            if (activationState_ != ActivationState::AwaitingUser) {
                return;
            }
            activationState_ = ActivationState::Starting;
            generation = activationGeneration_;
        }

        std::string error;
        const bool success = start(&error);

        {
            std::lock_guard<std::mutex> lock(activationMutex_);
            activationState_ = ActivationState::Idle;
            activationCompletedGeneration_ = generation;
            activationSucceeded_ = success;
            activationError_ = success ? std::string() : (error.empty()
                ? "Wayland screen capture could not be started."
                : error);
        }
        activationCv_.notify_all();
    }

    void declineActivationRequest()
    {
        {
            std::lock_guard<std::mutex> lock(activationMutex_);
            if (activationState_ != ActivationState::AwaitingUser) {
                return;
            }
            activationState_ = ActivationState::Idle;
            activationCompletedGeneration_ = activationGeneration_;
            activationSucceeded_ = false;
            activationError_ = "Wayland screen capture was declined. getPixelColor and getPixelRect require selecting a monitor.";
        }
        activationCv_.notify_all();
    }

#if defined(SMU_HAS_WAYLAND_SCREENCAST) && SMU_HAS_WAYLAND_SCREENCAST
private:
    static void OnStreamStateChanged(void* data, enum pw_stream_state, enum pw_stream_state state, const char* error)
    {
        auto* self = static_cast<Impl*>(data);
        if (state == PW_STREAM_STATE_ERROR) {
            std::lock_guard<std::mutex> lock(self->mutex_);
            self->active_ = false;
            self->status_ = std::string("Wayland ScreenCast stream failed: ") + (error ? error : "unknown error");
            self->frameCv_.notify_all();
        }
    }

    static void OnStreamParamChanged(void* data, std::uint32_t id, const struct spa_pod* param)
    {
        auto* self = static_cast<Impl*>(data);
        if (id != SPA_PARAM_Format || !param) {
            return;
        }
        spa_video_info_raw format{};
        if (spa_format_video_raw_parse(param, &format) < 0) {
            return;
        }
        std::lock_guard<std::mutex> lock(self->mutex_);
        self->videoFormat_ = format.format;
        self->streamWidth_ = static_cast<int>(format.size.width);
        self->streamHeight_ = static_cast<int>(format.size.height);
    }

    static void OnStreamProcess(void* data)
    {
        auto* self = static_cast<Impl*>(data);
        pw_buffer* pipeWireBuffer = pw_stream_dequeue_buffer(self->stream_);
        if (!pipeWireBuffer) {
            return;
        }
        spa_buffer* buffer = pipeWireBuffer->buffer;
        if (buffer && buffer->n_datas > 0 && buffer->datas[0].data) {
            const spa_data& data0 = buffer->datas[0];
            const spa_chunk* chunk = data0.chunk;
            const std::uint32_t offset = chunk ? chunk->offset : 0;
            const std::uint32_t size = chunk ? chunk->size : data0.maxsize;
            const std::uint8_t* source = static_cast<const std::uint8_t*>(data0.data) + offset;
            self->copyFrame(source, size, chunk ? chunk->stride : 0);
        }
        pw_stream_queue_buffer(self->stream_, pipeWireBuffer);
    }

    void copyFrame(const std::uint8_t* source, std::size_t sourceSize, int stride)
    {
        std::lock_guard<std::mutex> lock(mutex_);
        if (!source || streamWidth_ <= 0 || streamHeight_ <= 0) {
            return;
        }
        const bool bgr = videoFormat_ == SPA_VIDEO_FORMAT_BGRx || videoFormat_ == SPA_VIDEO_FORMAT_BGRA;
        const bool rgb = videoFormat_ == SPA_VIDEO_FORMAT_RGBx || videoFormat_ == SPA_VIDEO_FORMAT_RGBA;
        if (!bgr && !rgb) {
            status_ = "Wayland ScreenCast selected an unsupported pixel format.";
            return;
        }
        const std::size_t rowBytes = static_cast<std::size_t>(streamWidth_) * 4U;
        const std::size_t sourceStride = stride > 0 ? static_cast<std::size_t>(stride) : rowBytes;
        if (sourceStride < rowBytes || sourceSize < sourceStride * static_cast<std::size_t>(streamHeight_)) {
            status_ = "Wayland ScreenCast delivered an incomplete frame.";
            return;
        }
        frame_.resize(static_cast<std::size_t>(streamWidth_) * static_cast<std::size_t>(streamHeight_) * 3U);
        for (int y = 0; y < streamHeight_; ++y) {
            const std::uint8_t* row = source + static_cast<std::size_t>(y) * sourceStride;
            for (int x = 0; x < streamWidth_; ++x) {
                const std::uint8_t* pixel = row + static_cast<std::size_t>(x) * 4U;
                const std::size_t target = (static_cast<std::size_t>(y) * static_cast<std::size_t>(streamWidth_) +
                    static_cast<std::size_t>(x)) * 3U;
                frame_[target] = pixel[bgr ? 2 : 0];
                frame_[target + 1] = pixel[1];
                frame_[target + 2] = pixel[bgr ? 0 : 2];
            }
        }
        frameWidth_ = streamWidth_;
        frameHeight_ = streamHeight_;
        frameCv_.notify_all();
    }

    bool startPipeWire(int fd, std::uint32_t nodeId, std::string* errorMessage)
    {
        static std::once_flag pipeWireInit;
        std::call_once(pipeWireInit, [] { pw_init(nullptr, nullptr); });
        loop_ = pw_thread_loop_new("smu-wayland-screencast", nullptr);
        if (!loop_) {
            SetError(errorMessage, "Could not create the PipeWire capture loop.");
            return false;
        }
        if (pw_thread_loop_start(loop_) < 0) {
            SetError(errorMessage, "Could not start the PipeWire capture loop.");
            stopPipeWire();
            return false;
        }
        pw_thread_loop_lock(loop_);
        context_ = pw_context_new(pw_thread_loop_get_loop(loop_), nullptr, 0);
        core_ = context_ ? pw_context_connect_fd(context_, fd, nullptr, 0) : nullptr;
        if (!core_) {
            pw_thread_loop_unlock(loop_);
            SetError(errorMessage, "Could not connect PipeWire to the ScreenCast portal stream.");
            stopPipeWire();
            return false;
        }
        stream_ = pw_stream_new(core_, "SMU Wayland Screen Capture", pw_properties_new(
            PW_KEY_MEDIA_TYPE, "Video",
            PW_KEY_MEDIA_CATEGORY, "Capture",
            PW_KEY_MEDIA_ROLE, "Screen",
            nullptr));
        if (!stream_) {
            pw_thread_loop_unlock(loop_);
            SetError(errorMessage, "Could not create the PipeWire screen capture stream.");
            stopPipeWire();
            return false;
        }
        static const pw_stream_events streamEvents = [] {
            pw_stream_events events{};
            events.version = PW_VERSION_STREAM_EVENTS;
            events.state_changed = OnStreamStateChanged;
            events.param_changed = OnStreamParamChanged;
            events.process = OnStreamProcess;
            return events;
        }();
        pw_stream_add_listener(stream_, &streamListener_, &streamEvents, this);

        std::uint8_t buffer[1024];
        spa_pod_builder builder = SPA_POD_BUILDER_INIT(buffer, sizeof(buffer));
        const spa_pod* params[1] = {
            static_cast<const spa_pod*>(spa_pod_builder_add_object(
                &builder,
                SPA_TYPE_OBJECT_Format, SPA_PARAM_EnumFormat,
                SPA_FORMAT_mediaType, SPA_POD_Id(SPA_MEDIA_TYPE_video),
                SPA_FORMAT_mediaSubtype, SPA_POD_Id(SPA_MEDIA_SUBTYPE_raw),
                SPA_FORMAT_VIDEO_format, SPA_POD_CHOICE_ENUM_Id(4,
                    SPA_VIDEO_FORMAT_BGRx, SPA_VIDEO_FORMAT_BGRA,
                    SPA_VIDEO_FORMAT_RGBx, SPA_VIDEO_FORMAT_RGBA),
                0))
        };
        const int result = pw_stream_connect(
            stream_, PW_DIRECTION_INPUT, nodeId,
            static_cast<pw_stream_flags>(PW_STREAM_FLAG_AUTOCONNECT | PW_STREAM_FLAG_MAP_BUFFERS), params, 1);
        pw_thread_loop_unlock(loop_);
        if (result < 0) {
            SetError(errorMessage, "Could not connect to the selected PipeWire screen stream: " + std::string(spa_strerror(result)));
            stopPipeWire();
            return false;
        }
        return true;
    }

    void stopPipeWire()
    {
        if (!loop_) {
            return;
        }
        pw_thread_loop_lock(loop_);
        if (stream_) {
            spa_hook_remove(&streamListener_);
            pw_stream_destroy(stream_);
            stream_ = nullptr;
        }
        if (core_) {
            pw_core_disconnect(core_);
            core_ = nullptr;
        }
        if (context_) {
            pw_context_destroy(context_);
            context_ = nullptr;
        }
        pw_thread_loop_unlock(loop_);
        pw_thread_loop_stop(loop_);
        pw_thread_loop_destroy(loop_);
        loop_ = nullptr;
    }

    static void CloseSession(DBusConnection* connection, const std::string& sessionHandle)
    {
        if (!connection || sessionHandle.empty()) {
            return;
        }
        DBusMessage* request = dbus_message_new_method_call(
            kPortalBusName, sessionHandle.c_str(), kSessionInterface, "Close");
        if (!request) {
            return;
        }
        DBusError error;
        dbus_error_init(&error);
        DBusMessage* reply = dbus_connection_send_with_reply_and_block(connection, request, 1000, &error);
        dbus_message_unref(request);
        if (reply) {
            dbus_message_unref(reply);
        }
        dbus_error_free(&error);
    }

    mutable std::mutex mutex_;
    std::condition_variable frameCv_;
    bool active_ = false;
    std::string status_ = "Wayland ScreenCast is inactive.";
    std::vector<std::uint8_t> frame_;
    int frameWidth_ = 0;
    int frameHeight_ = 0;
    int streamWidth_ = 0;
    int streamHeight_ = 0;
    spa_video_format videoFormat_ = SPA_VIDEO_FORMAT_UNKNOWN;
    DBusConnection* portalConnection_ = nullptr;
    std::string sessionHandle_;
    pw_thread_loop* loop_ = nullptr;
    pw_context* context_ = nullptr;
    pw_core* core_ = nullptr;
    pw_stream* stream_ = nullptr;
    spa_hook streamListener_{};
#else
private:
    mutable std::mutex mutex_;
    bool active_ = false;
    std::string status_ = "Wayland ScreenCast is unavailable in this build.";
    std::vector<std::uint8_t> frame_;
    int frameWidth_ = 0;
    int frameHeight_ = 0;
#endif

private:
    enum class ActivationState {
        Idle,
        AwaitingUser,
        Starting
    };

    mutable std::mutex activationMutex_;
    std::condition_variable activationCv_;
    ActivationState activationState_ = ActivationState::Idle;
    unsigned long long activationGeneration_ = 0;
    unsigned long long activationCompletedGeneration_ = 0;
    bool activationSucceeded_ = false;
    std::string activationError_;
};

WaylandScreenCast& WaylandScreenCast::instance()
{
    static WaylandScreenCast screenCast;
    return screenCast;
}

WaylandScreenCast::WaylandScreenCast()
    : impl_(new Impl())
{
}

WaylandScreenCast::~WaylandScreenCast()
{
    stop();
    delete impl_;
}

bool WaylandScreenCast::isSupported() const
{
    return impl_->supported();
}

bool WaylandScreenCast::isActive() const
{
    return impl_->active();
}

std::string WaylandScreenCast::status() const
{
    return impl_->status();
}

bool WaylandScreenCast::start(std::string* errorMessage)
{
    if (errorMessage) {
        errorMessage->clear();
    }
    return impl_->start(errorMessage);
}

void WaylandScreenCast::stop()
{
    impl_->stop();
}

bool WaylandScreenCast::requestActivationForScript(const std::function<bool()>& isCancelled, std::string* errorMessage)
{
    if (errorMessage) {
        errorMessage->clear();
    }
    return impl_->requestActivationForScript(isCancelled, errorMessage);
}

bool WaylandScreenCast::hasPendingActivationRequest() const
{
    return impl_->hasPendingActivationRequest();
}

void WaylandScreenCast::approveActivationRequest()
{
    impl_->approveActivationRequest();
}

void WaylandScreenCast::declineActivationRequest()
{
    impl_->declineActivationRequest();
}

std::optional<ScreenBounds> WaylandScreenCast::selectedMonitorBounds() const
{
    return impl_->bounds();
}

std::optional<PixelColor> WaylandScreenCast::sample(int x, int y, std::string* errorMessage) const
{
    if (errorMessage) {
        errorMessage->clear();
    }
    return impl_->sample(x, y, errorMessage);
}

} // namespace smu::platform::linux
