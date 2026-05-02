#include "input_actions.h"

#include "../platform/input_backend.h"

namespace smu::app {

bool IsKeyPressed(core::KeyCode key)
{
    if (auto backend = platform::GetInputBackend()) {
        return backend->isKeyPressed(key);
    }
    return false;
}

void HoldKey(core::KeyCode key, bool extended)
{
    if (auto backend = platform::GetInputBackend()) {
        backend->holdKey(key, extended);
    }
}

void ReleaseKey(core::KeyCode key, bool extended)
{
    if (auto backend = platform::GetInputBackend()) {
        backend->releaseKey(key, extended);
    }
}

void HoldKeyBinded(core::KeyCode combinedKey)
{
    if (auto backend = platform::GetInputBackend()) {
        backend->holdKeyChord(combinedKey);
    }
}

void ReleaseKeyBinded(core::KeyCode combinedKey)
{
    if (auto backend = platform::GetInputBackend()) {
        backend->releaseKeyChord(combinedKey);
    }
}

void MoveMouse(int dx, int dy)
{
    if (auto backend = platform::GetInputBackend()) {
        backend->moveMouse(dx, dy);
    }
}

void MouseWheel(int delta)
{
    if (auto backend = platform::GetInputBackend()) {
        backend->mouseWheel(delta);
    }
}

} // namespace smu::app
