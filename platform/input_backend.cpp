#include "input_backend.h"

#include <mutex>
#include <utility>

namespace smu::platform {
namespace {
std::mutex g_inputBackendMutex;
std::shared_ptr<InputBackend> g_inputBackend;
}

std::shared_ptr<InputBackend> GetInputBackend()
{
    std::lock_guard<std::mutex> lock(g_inputBackendMutex);
    return g_inputBackend;
}

void SetInputBackend(std::shared_ptr<InputBackend> backend)
{
    std::lock_guard<std::mutex> lock(g_inputBackendMutex);
    g_inputBackend = std::move(backend);
}

} // namespace smu::platform
