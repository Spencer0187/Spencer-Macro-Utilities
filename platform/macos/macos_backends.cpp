#include "macos_backends.h"

#include "input_cgevent.h"
#include "process_macos.h"

#include "../input_backend.h"
#include "../network_backend.h"
#include "../process_backend.h"

namespace smu::platform::macos {

void InitializeMacOSPlatformBackends()
{
    smu::platform::SetInputBackend(CreateMacosInputBackend());
    smu::platform::SetProcessBackend(CreateMacosProcessBackend());
    smu::platform::SetNetworkLagBackend(nullptr);
}

} // namespace smu::platform::macos
