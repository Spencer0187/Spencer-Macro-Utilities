#include "window_backend.h"

#include <mutex>
#include <utility>

namespace smu::platform {
namespace {
std::mutex g_windowBackendMutex;
std::shared_ptr<WindowBackend> g_windowBackend;
}

std::shared_ptr<WindowBackend> GetWindowBackend()
{
    std::lock_guard<std::mutex> lock(g_windowBackendMutex);
    return g_windowBackend;
}

void SetWindowBackend(std::shared_ptr<WindowBackend> backend)
{
    std::lock_guard<std::mutex> lock(g_windowBackendMutex);
    g_windowBackend = std::move(backend);
}

} // namespace smu::platform
