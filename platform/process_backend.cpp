#include "process_backend.h"

#include <mutex>
#include <utility>

namespace smu::platform {
namespace {
std::mutex g_processBackendMutex;
std::shared_ptr<ProcessBackend> g_processBackend;
}

std::shared_ptr<ProcessBackend> GetProcessBackend()
{
    std::lock_guard<std::mutex> lock(g_processBackendMutex);
    return g_processBackend;
}

void SetProcessBackend(std::shared_ptr<ProcessBackend> backend)
{
    std::lock_guard<std::mutex> lock(g_processBackendMutex);
    g_processBackend = std::move(backend);
}

} // namespace smu::platform
